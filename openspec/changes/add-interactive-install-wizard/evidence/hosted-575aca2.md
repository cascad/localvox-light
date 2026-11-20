# Hosted delivery and Mac retry — 2026-11-20

## Published installer versus pushed source

GitHub REST was read after the owner's push of `575aca2`. Latest published release
is still **v0.2.1**, published 2026-11-15, with 14 assets. Its manifest still contains
the Vosk mirror `huggingface.co/mychen76/vosk-models/...`, not the official source
fixed in `5d4d1f7`. Ordinary bootstrap retry sees an intact v0.2.1 bundle and reuses
it; pushing main alone does not publish a release. After a corrected release is
published, the existing installation must use `--update` / `-Update`.

Owner's Mac log is manual evidence of reaching the plan and Vosk download, with
mpv reported ready. The previous mpv error was not retained; its cause is unknown.
No player, microphone, daemon or completed model-install acceptance is claimed.

## Actual hosted results on 575aca2

- [Build 44603460238](https://github.com/cascad/localvox-light/actions/runs/44603460238):
  public source check, both macOS targets and both Linux targets **success**.
  Windows **failed** in Delivery contracts; release publication **skipped**.
- Windows job `135840882917`: 55 delivery tests, 2 failures. Both were PowerShell 7
  errors from deliberately truncated/HTTP 503 transfers: the inherited renderer
  emitted ANSI into redirected stderr. Progress/cancellation terminal tests passed.
- [Interface 44603460318](https://github.com/cascad/localvox-light/actions/runs/44603460318),
  job `135840761972`: UI/client/shared workspace/player steps passed. Chocolatey
  returned HTTP **503** while resolving ffmpeg 9.0.2, installing no package. Its
  one-line step did not stop on native failure, so later FLAC tests failed with
  `program not found`. This is a failed dependency installation, not a codec defect.

## Local corrections and executed checks

- With NO_COLOR removed, TERM/FORCE_COLOR set and PSStyle forced to Ansi, the new
  regression reproduced both redirected-error failures. Bootstrap now explicitly
  selects PlainText for a redirected PS7 host, including its later error rendering.
  PS5 remains compatible; ordinary terminal styling is retained.
- The ffmpeg prerequisite now has the same fail-fast native-command policy as
  media tests, and verifies `ffmpeg -version` before they start. No retries hide
  the original Chocolatey 503, no codec assertions or timeouts were weakened.
- Bootstrap delivery: **7 passed / 1 skipped**, including the forced-ANSI regression,
  PS5/PS7 known/unknown transfers, Git Bash, ConPTY cancellation and error cleanup.
  The skip is native Unix terminal acceptance on the Windows host.
- CI delivery contracts: **4 passed / 1 skipped**. The workflow native-failure
  regression includes the new prerequisite block. The full native archive was not
  supplied to that optional check.
- Real ffmpeg generated-PCM codec tests: **2 passed**, sample/range preservation and
  reader ownership/recovered capture. No user audio, archive or daemon involved.
- Fresh source probe: **9/9** public HEAD/bounded GET checks passed. No complete
  model download, hash verification or inference is inferred from this result.
- actionlint, strict main specs/change, check-openspec and diff checks passed.

These fixes are local pending the owner's next push. New hosted runs and a new
published release are **not-run**; the successful Mac CI jobs above do not stand
for a completed installation on the owner's Mac.
