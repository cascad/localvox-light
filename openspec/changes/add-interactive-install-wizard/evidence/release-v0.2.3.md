# Release v0.2.3 — preparation, 2026-11-20

The unpublished v0.2.2 candidate exposed a real macOS cancellation defect. No
existing tag is moved or deleted. The next release uses workspace/desktop/UI
version 0.2.3 and the supported workflow_dispatch publication path: all five
platform builds and the reusable Interface workflow must pass before the job
creates the tag and publishes the assets.

## Cancellation correction

The shell bootstrap ran curl asynchronously and only installed an EXIT trap.
On native macOS, Ctrl+C did not make the non-interactive parent exit while curl
continued. It now explicitly traps INT → exit 130 and TERM → exit 143; the existing
EXIT cleanup terminates/waits only for its owned curl before removing staging.
No cancellation assertion, timeout or transfer-integrity check was weakened.
The actual ConPTY cancellation/progress test passed locally after this correction.

Hosted native macOS confirmation, public release and public bootstrap verification
are pending. A successful Git Bash run is not described as macOS acceptance.

## Publication and verification, 2026-11-21

The pending status above records preparation, not the final hosted result.
[Release run 44810435050](https://github.com/cascad/localvox-light/actions/runs/44810435050)
passed all five targets, public model-source checks, the reusable Interface suite
(including 81 browser e2e and 213 TUI unit tests), and publication. Native macOS
bootstrap progress/cancellation tests passed. [v0.2.3](https://github.com/cascad/localvox-light/releases/tag/v0.2.3)
was published at 2026-11-20T20:58:07Z from 409e482b3accfaf6f958cd7e837683342b0acdb3
and became latest. The separate main Interface run 44810435052 also passed.

The separate [main Build run](https://github.com/cascad/localvox-light/actions/runs/44810435054)
failed on Windows: outcome_failure read legacy progress while an append held its
brief Windows file lock, then unwrapped the empty read. The next test inherited
the poisoned fixture mutex. Release checks passed on the same source; this main
failure is not rewritten as green. The test synchronization correction is in the
[0.2.4 follow-up](release-v0.2.4.md).

Publication downloaded reusable UI evidence along with product artifacts and
included trace.zip. On 2026-11-21 only that unintended asset was removed, after
retaining a digest-verified backup under target/review on D:. Product archives,
checksums and the release tag were not replaced. The corrected collection is
covered in the next candidate.

All 14 intended public assets were verified: the five archive digests match their
public checksum files, and all four published installer scripts match the tagged
source and asset digests. The full Windows ZIP was downloaded separately and its
actual SHA-256 verified; bundle.json identifies v0.2.3 and scripts/models.json uses
the official alphacephei Vosk URL. The public install.sh passed bash -n.
The actual public archive passed check-release-install.py: native doctor/API,
missing-ASR exit 2, read-only checks, bundle integrity, reconciliation preserving
settings and refusal to run a wizard without a terminal.

The original exact public Windows one-liner did not reach the wizard within the
private harness's 300 s deadline: about 50/116 MiB had arrived. Direct download of
the identical public ZIP took 4.09 s. Investigation found the bootstrap's fixed
100 ms sleep after each asynchronous read, often only a TLS record. This is a
confirmed bootstrap latency defect, not a speech-model or network throughput
measurement. The failed smoke is retained, not described as successful. The
corrected local bootstrap with the same public bundle reached 100% in 9 s, opened
the real wizard, verified native version/bundle identity, and exited on Esc with
no install/model/config/PATH changes. This last run used corrected local source,
not the still-published v0.2.3 bootstrap. Delivery of that correction is tracked
in 9.2.

No physical capture, real OS dependency install or speech/LLM model install is
claimed by these fixture checks. Private transcripts and ZIPs are ignored under
target/review/release-v0.2.3; no user archive or running daemon was used.
