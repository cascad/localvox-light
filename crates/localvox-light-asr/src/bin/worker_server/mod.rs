//! Sequential private stdio server. The parent owns each request's execution lease.
use anyhow::{ensure, Context, Result};
use localvox_light_core::{
    cook::{self, AsrRuntime, CookOutcome, CookParams},
    jobs::asr_worker::{Reply, Request, PROTOCOL},
    progress::{self, Stage, StageState},
};
use std::{
    io::{BufRead, Read, Write},
    path::Path,
};

fn execute(
    root: &Path,
    request: &Request,
    runtime: &mut AsrRuntime,
    fixture: Option<&Path>,
) -> Result<serde_json::Value> {
    ensure!(
        request.protocol == PROTOCOL,
        "ASR request protocol mismatch"
    );
    let session = request.session.canonicalize().context("ASR session")?;
    ensure!(
        session.parent() == Some(root),
        "ASR session must be directly inside owned sessions root"
    );
    ensure!(
        !request.prefix || !request.force,
        "prefix cannot force final recognition"
    );
    ensure!(
        request.prefix || request.input.is_none(),
        "final request cannot use a prefix horizon"
    );
    let params = CookParams {
        model_dir: request.model_dir.clone(),
        force: request.force,
        ..Default::default()
    };
    #[cfg(not(feature = "asr-fixtures"))]
    ensure!(
        fixture.is_none(),
        "synthetic ASR is unavailable in this build"
    );
    if request.prefix {
        #[cfg(feature = "asr-fixtures")]
        if let Some(spec) = fixture {
            return Ok(serde_json::to_value(cook::transcribe_warm_fixture(
                &session,
                &params,
                spec,
                request.input.as_ref(),
                runtime,
            )?)?);
        }
        return Ok(serde_json::to_value(cook::transcribe_warm(
            &session,
            &params,
            request.input.as_ref(),
            runtime,
        )?)?);
    }
    #[cfg(feature = "asr-fixtures")]
    let result = if let Some(spec) = fixture {
        cook::cook_session_asr_warm_fixture(&session, &params, spec, runtime)
    } else {
        cook::cook_session_asr_warm(&session, &params, runtime)
    };
    #[cfg(not(feature = "asr-fixtures"))]
    let result = cook::cook_session_asr_warm(&session, &params, runtime);
    let value = match result? {
        CookOutcome::Done {
            version_id,
            lines,
            wall_sec,
            ..
        } => {
            progress::mark(
                &session,
                Stage::Transcribe,
                StageState::Done,
                Some(&format!("{lines} строк")),
            );
            serde_json::json!({"version_id":version_id,"lines":lines,"wall_sec":wall_sec})
        }
        CookOutcome::Skipped { existing_version } => {
            progress::mark(
                &session,
                Stage::Transcribe,
                StageState::Done,
                Some("Распознавание уже выполнено"),
            );
            serde_json::json!({"existing_version":existing_version})
        }
        CookOutcome::Empty => {
            localvox_light_core::processing::record(
                &session,
                localvox_light_core::processing::TRANSCRIPT,
                "no-audio",
                localvox_light_core::processing::Outcome::Nothing,
                Some("в записи нет звука".into()),
                None,
            )?;
            progress::mark(
                &session,
                Stage::Transcribe,
                StageState::Skipped,
                Some("в записи нет звука"),
            );
            serde_json::json!({"empty":true})
        }
    };
    Ok(value)
}

pub fn serve(work_dir: &Path, fixture: Option<&Path>) -> Result<()> {
    let root = work_dir
        .join("sessions")
        .canonicalize()
        .context("resident sessions root")?;
    let mut runtime = AsrRuntime::default();
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut previous = None;
    loop {
        let mut frame = Vec::new();
        let n = (&mut stdin).take(65538).read_until(b'\n', &mut frame)?;
        if n == 0 {
            return Ok(());
        }
        ensure!(
            n <= 65537 && frame.last() == Some(&b'\n'),
            "ASR request frame exceeds its limit or is incomplete"
        );
        let request: Request = serde_json::from_slice(&frame)?;
        ensure!(
            previous.is_none_or(|id| request.id > id),
            "ASR request id is not increasing"
        );
        previous = Some(request.id);
        let before = runtime.loads();
        let result = execute(&root, &request, &mut runtime, fixture).map_err(|e| format!("{e:#}"));
        if result.is_err() {
            runtime.clear();
        }
        let reply = Reply {
            protocol: PROTOCOL.into(),
            id: request.id,
            result,
            model_loads: runtime.loads() - before,
        };
        let bytes = serde_json::to_vec(&reply)?;
        ensure!(
            bytes.len() <= 8 * 1024 * 1024,
            "ASR reply exceeds its limit"
        );
        stdout.write_all(&bytes)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
}
