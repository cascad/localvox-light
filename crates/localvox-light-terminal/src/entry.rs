use clap::{CommandFactory, FromArgMatches, Parser};
use std::{net::SocketAddr, path::Path};

#[derive(Parser)]
#[command(
    name = "localvox-tui",
    version,
    about = "LocalVox — терминальное рабочее пространство"
)]
struct Cli {
    #[arg(long, env = "LOCALVOX_TUI_URL")]
    address: Option<String>,
    /// Register vox in the current user's PATH, without starting the daemon.
    #[arg(long, conflicts_with = "check_command")]
    install_command: bool,
    /// Read-only check of vox command availability and registration.
    #[arg(long)]
    check_command: bool,
    /// Read from the environment; never put the access token into process arguments.
    #[arg(skip)]
    token: String,
    #[arg(long)]
    ascii: bool,
    #[arg(long)]
    no_color: bool,
    #[arg(long)]
    color16: bool,
    #[arg(long)]
    light: bool,
    /// Optional JSON command keymap; ordinary text and paste remain unchanged.
    #[arg(long, env = "LOCALVOX_TUI_KEYMAP")]
    keymap: Option<std::path::PathBuf>,
}

pub fn run(vox: bool) -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let install = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Нет каталога установки"))?;
    let maintenance = std::env::args_os().skip(1).any(|a| {
        matches!(
            a.to_str(),
            Some("--install-command" | "--check-command" | "--help" | "-h" | "--version" | "-V")
        )
    });
    if vox && !maintenance {
        portable(install)?;
    }
    let mut cli = Cli::from_arg_matches(
        &Cli::command()
            .name(if vox { "vox" } else { "localvox-tui" })
            .get_matches(),
    )?;
    if cli.install_command || cli.check_command {
        let result = if cli.install_command {
            crate::command_path::install(install)
        } else {
            crate::command_path::check(install)
        };
        match result {
            Ok(report) => {
                println!("{}", report.message);
                std::process::exit(report.code);
            }
            Err(error) => {
                eprintln!("vox: {error:#}");
                std::process::exit(2);
            }
        }
    }
    cli.token = std::env::var("LOCALVOX_API_TOKEN").unwrap_or_default();
    let keymap = cli
        .keymap
        .as_deref()
        .map(crate::keymap::Keymap::load)
        .transpose()?
        .unwrap_or_default();
    let address = cli.address.unwrap_or_else(|| default_address(vox));
    let client = localvox_light_client::Client::new(&address, cli.token)?;
    let app = crate::app::App {
        preferences: crate::app::UiPreferences {
            keymap,
            ascii: cli.ascii,
            monochrome: cli.no_color || std::env::var_os("NO_COLOR").is_some(),
            color16: cli.color16
                || !(std::env::var("COLORTERM")
                    .is_ok_and(|v| matches!(v.as_str(), "truecolor" | "24bit"))
                    || std::env::var_os("WT_SESSION").is_some()),
            light: cli.light,
            ..Default::default()
        },
        ..Default::default()
    };
    crate::runtime::run(client, app)
}

fn portable(install: &Path) -> anyhow::Result<()> {
    std::env::set_current_dir(install)?;
    for (name, override_values) in [(".env.defaults", false), (".env", true)] {
        let path = install.join(name);
        if path.is_file() {
            if override_values {
                dotenvy::from_path_override(path)?;
            } else {
                dotenvy::from_path(path)?;
            }
        }
    }
    Ok(())
}

fn default_address(vox: bool) -> String {
    let bind = if vox {
        std::env::var("LOCALVOX_API_BIND").ok()
    } else {
        None
    };
    bind.map(|value| {
        let address = value
            .parse::<SocketAddr>()
            .map(|mut a| {
                if a.ip().is_unspecified() {
                    a.set_ip(if a.is_ipv4() {
                        "127.0.0.1".parse().unwrap()
                    } else {
                        "::1".parse().unwrap()
                    });
                }
                a.to_string()
            })
            .unwrap_or(value);
        format!("http://{address}")
    })
    .unwrap_or_else(|| "http://127.0.0.1:3017".into())
}
