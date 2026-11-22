use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::{net::SocketAddr, path::Path};

#[derive(Parser)]
#[command(
    name = "localvox-tui",
    version,
    args_conflicts_with_subcommands = true,
    about = "LocalVox — терминальное рабочее пространство"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Maintenance>,
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

#[derive(Subcommand)]
enum Maintenance {
    /// Add optional components or change the installation using the same wizard.
    Setup {
        /// Check the current installation without changes or model inference.
        #[arg(long)]
        check: bool,
    },
}

fn setup_command(install: &Path, check: bool) -> anyhow::Result<std::process::Command> {
    let installer = install.join(if cfg!(windows) {
        "localvox-install.exe"
    } else {
        "localvox-install"
    });
    anyhow::ensure!(installer.is_file(), "Мастер установки не найден рядом с vox. Повторите команду установки из README; настройки и готовые модели сохранятся.");
    let mut command = std::process::Command::new(installer);
    command
        .current_dir(install)
        .arg("--install-dir")
        .arg(install);
    if check {
        command.arg("--check");
    }
    Ok(command)
}

pub fn run(vox: bool) -> anyhow::Result<()> {
    let executable = std::env::current_exe()?;
    let install = executable
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Нет каталога установки"))?;
    let maintenance = std::env::args_os().nth(1).is_some_and(|a| a == "setup")
        || std::env::args_os().skip(1).any(|a| {
            matches!(
                a.to_str(),
                Some(
                    "--install-command" | "--check-command" | "--help" | "-h" | "--version" | "-V"
                )
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
    if let Some(Maintenance::Setup { check }) = cli.command {
        let status = setup_command(install, check)?.status()?;
        // Doctor's 0/1/2 are meaningful; do not turn a warning into a generic
        // launcher failure or repeat diagnostics already printed by the wizard.
        std::process::exit(status.code().unwrap_or(1));
    }
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
        focus: 0,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_is_a_maintenance_command_without_workspace_options() {
        assert!(matches!(
            Cli::try_parse_from(["vox", "setup"]).unwrap().command,
            Some(Maintenance::Setup { check: false })
        ));
        assert!(matches!(
            Cli::try_parse_from(["vox", "setup", "--check"])
                .unwrap()
                .command,
            Some(Maintenance::Setup { check: true })
        ));
        assert!(Cli::try_parse_from(["vox", "--address", "http://remote.test", "setup"]).is_err());
    }

    #[test]
    fn setup_executes_the_sibling_installer_in_the_same_directory_with_inherited_terminal() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("installation with spaces");
        std::fs::create_dir(&install).unwrap();
        assert!(setup_command(&install, false).is_err());
        let installer = install.join(if cfg!(windows) {
            "localvox-install.exe"
        } else {
            "localvox-install"
        });
        std::fs::write(&installer, b"presence fixture, never executed").unwrap();
        let command = setup_command(&install, true).unwrap();
        assert_eq!(command.get_program(), installer.as_os_str());
        assert_eq!(command.get_current_dir(), Some(install.as_path()));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![
                std::ffi::OsStr::new("--install-dir"),
                install.as_os_str(),
                std::ffi::OsStr::new("--check")
            ]
        );
    }
}
