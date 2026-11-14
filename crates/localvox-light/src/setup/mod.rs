//! Terminal setup shares runtime discovery and manifest downloaders with the product.
pub mod bundle;
pub mod execute;
pub mod install;
pub mod pending;
pub mod plan;
pub mod probe;
pub mod tools;

use anyhow::Result;
use localvox_light_core::{
    doctor::{self, Layout, State},
    Cli,
};

pub fn report(layout: &Layout) -> Vec<doctor::Finding> {
    let mut findings = doctor::inspect(layout);
    findings.extend(probe::ollama_findings(layout));
    findings
}
pub fn print_report(findings: &[doctor::Finding]) {
    for f in findings {
        println!(
            "  [{}] {} · {}\n         {}",
            f.state.mark(),
            f.id,
            f.what,
            f.detail
        );
    }
}
pub fn run(cli: &Cli) -> Result<i32> {
    let layout = Layout::from_env(
        localvox_light_core::cli::normalized_model_path(cli).into(),
        cli.audio_dir.clone().into(),
    );
    if cli.setup_check {
        let findings = report(&layout);
        print_report(&findings);
        return Ok(match doctor::worst(&findings) {
            State::Ok => 0,
            State::Warn => 1,
            State::Fail => 2,
        });
    }
    let exe = std::env::current_exe()?;
    let install = exe.parent().unwrap();
    self::install::run(
        install,
        install,
        self::install::Options {
            discovery: Some(layout),
            settings_path: Some(localvox_light_core::env_file::resolve_path()),
            ..Default::default()
        },
    )
}
