//! Terminal setup shares runtime discovery and manifest downloaders with the product.
pub mod execute;
pub mod plan;
pub mod probe;
pub mod prompt;

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
    let findings = report(&layout);
    print_report(&findings);
    if cli.setup_check {
        return Ok(match doctor::worst(&findings) {
            State::Ok => 0,
            State::Warn => 1,
            State::Fail => 2,
        });
    }
    let exe = std::env::current_exe()?;
    let install = exe.parent().unwrap();
    let env_path = localvox_light_core::env_file::resolve_path();
    let settings = localvox_light_core::env_file::Snapshot::read(&env_path)?;
    let plan = prompt::choose(
        &layout,
        install,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
    )?;
    if let Some(plan) = plan {
        execute::apply(&plan, &layout, install, &settings)?;
        println!("\nНастройка завершена. Повторная проверка:");
        // Re-probe the explicit new layout without mutating process environment.
        let mut updated = layout.clone();
        for (key, value) in &plan.changes {
            if let Some(value) = value {
                for c in plan::Component::ALL {
                    if c.key() == key {
                        c.set(&mut updated, value.into());
                    }
                }
                if key == "LOCALVOX_LLM_BASE_URL" {
                    updated.llm_base_url = value.clone();
                }
                if key == "LOCALVOX_LLM_MODEL" {
                    updated.llm_model = value.clone();
                }
            }
        }
        print_report(&report(&updated));
        println!("--setup-check — проверить снова; --setup — изменить выбор.");
    }
    Ok(0)
}
