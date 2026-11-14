//! Safe visual prototype. No dotenv loading, discovery, downloader, daemon or executor.
use localvox_setup_ui::{choose, demonstrate, Feature, ViewData};
use std::io::{self, IsTerminal};

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("LocalVox · демо установщика, без установки\n\nlocalvox-setup-preview [--scenario fresh|existing]\n\nfresh — первая установка, existing — всё уже готово (фиктивные данные).\n↑/↓ — выбор, Пробел — галочка, Enter — дальше, Esc — назад / отмена.");
        return Ok(());
    }
    let scenario = match args.as_slice() {
        [] => "fresh",
        [flag, value] if flag == "--scenario" && matches!(value.as_str(), "fresh" | "existing") => {
            value
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Используйте --scenario fresh|existing (или --help)",
            ))
        }
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Откройте демо в интерактивном терминале. Установка не выполнялась.",
        ));
    }
    let home = std::env::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"));
    let program_dir = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Local"))
            .join("Programs")
            .join("LocalVox")
    } else if cfg!(target_os = "macos") {
        home.join("Applications/LocalVox")
    } else {
        home.join(".local/share/localvox")
    };
    // A public preset supplies only a display value; no private .env or system probe is read.
    let summary_model = include_str!("../../../.env.defaults")
        .lines()
        .find_map(|line| line.strip_prefix("LOCALVOX_LLM_MODEL="))
        .unwrap_or("qwen3.5:9b")
        .to_string();
    let existing = scenario == "existing";
    let data = ViewData {
        program_dir: program_dir.display().to_string(),
        models_dir: home.join(".localvox").join("models").display().to_string(),
        summary_model,
        core_ready: existing,
        installed: if existing {
            vec![
                Feature::Speakers,
                Feature::Summary,
                Feature::Links,
                Feature::Player,
            ]
        } else {
            vec![]
        },
        ollama_ready: existing,
        preview: true,
        initial: None,
        available_model_paths: Default::default(),
    };
    if let Some(selected) = choose(&data)? {
        demonstrate(&selected)?;
    }
    Ok(())
}
