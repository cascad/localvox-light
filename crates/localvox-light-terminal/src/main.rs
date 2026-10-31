use clap::Parser;

#[derive(Parser)]
#[command(
    name = "localvox-tui",
    version,
    about = "LocalVox — терминальное рабочее пространство"
)]
struct Cli {
    #[arg(
        long,
        default_value = "http://127.0.0.1:3017",
        env = "LOCALVOX_TUI_URL"
    )]
    address: String,
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
}

fn main() -> anyhow::Result<()> {
    let mut cli = Cli::parse();
    cli.token = std::env::var("LOCALVOX_API_TOKEN").unwrap_or_default();
    let client = localvox_light_client::Client::new(&cli.address, cli.token)?;
    let app = localvox_light_terminal::app::App {
        ascii: cli.ascii,
        monochrome: cli.no_color || std::env::var_os("NO_COLOR").is_some(),
        color16: cli.color16
            || !(std::env::var("COLORTERM")
                .is_ok_and(|v| matches!(v.as_str(), "truecolor" | "24bit"))
                || std::env::var_os("WT_SESSION").is_some()),
        light: cli.light,
        ..Default::default()
    };
    localvox_light_terminal::runtime::run(client, app)
}
