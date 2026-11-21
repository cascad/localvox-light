# Release v0.2.4 — published and verified, 2026-11-21

SETUP-009 (delta), SETUP-003 and PIPE-008/011 test evidence. This follows the
[published 0.2.3 verification](release-v0.2.3.md); existing tags are not moved.

## Corrections

- Windows bootstrap now waits for task completion with a bounded
  [Task.WaitAny](https://learn.microsoft.com/en-us/dotnet/api/system.threading.tasks.task.waitany?view=netframework-4.8.1)
  rather than always sleeping 100 ms after each read. It wakes immediately when
  I/O completes, retains progress refresh and the 60 s no-response guard, and
  unwraps the original result/error through the existing GetAwaiter path.
- The publication job downloads localvox-* artifacts only and collects product
  archives/checksums by their localvox- prefix. Browser traces remain in Actions
  evidence, not in public release attachments.
- The Windows outcome fixture observes journal start and a readable legacy stage
  within its existing 5 s deadline. The exact pre-acceptance Running assertion
  uses that observation; a second read racing an append no longer unwraps None.
  Production queue, progress and retry behavior are unchanged by this test fix.
- Workspace, 16 local Cargo packages, desktop and UI versions are 0.2.4.

## Local executed checks

- Real workflow assembly with five archive/checksum fixture pairs plus nested
  browser trace: red on old workflow (trace.zip leaked), green after filtering.
  PackageContracts: 13 passed / 1 skipped (explicit archive not supplied there).
- Windows task-chain regression: old PS7 wait took 10940 ms for 100 tasks and
  failed the 6000 ms bound; corrected PS5 and PS7 pass without changing the bound.
  An initial fixture needed to suppress the .NET Framework void-task return before
  parsing JSON; that harness error is distinct from the PS7 red result.
- Full bootstrap fixtures: 9 passed / 1 native-Unix-only skipped on Windows,
  including PS5/PS7 errors, versions/reuse, bounds, hash failure, traversal,
  readable progress and real ConPTY cancellation. Full coordinator library:
  16 passed / 11 explicitly ignored worker/hardware-fixture tests, 2 test threads.
- Corrected local bootstrap + actual public 0.2.3 Windows ZIP in ConPTY:
  116.3 MiB downloaded in 9 s, SHA-256 verified, native wizard reached, bundle
  identity and binary version checked, Esc -> exit 0. No selected installation,
  model, configuration or PATH changes. This is not a published 0.2.4 smoke.

At the preparation boundary, native hosted checks and public v0.2.4 delivery were
pending. Hardware acceptance and installation of real OS packages/models remain
not-run after the delivery verification below.

Strict main specs (15/15), strict change, check-openspec.py (15 capabilities,
88 requirements, 188 scenarios), scoped rustfmt, actionlint, metadata/package
version guard and diff checks passed before commit. No existing tag is rewritten.

## Hosted publication and public delivery

Candidate: 97d33e4095b1ed4472c73dc011f188ebe3ee87ae. The supported manual publication
workflow created the new tag only after its checks succeeded; no older release
or tag was repointed. [v0.2.4](https://github.com/cascad/localvox-light/releases/tag/v0.2.4)
was published at 2026-11-21T07:13:13Z and became latest.

- [Release run 44876277226](https://github.com/cascad/localvox-light/actions/runs/44876277226):
  model sources, reusable Interface, all five platforms and publication — success.
- Windows x64 job 136761602353, Linux x64 136761602398, Linux ARM64 136761602368,
  macOS Apple Silicon 136761602314 and macOS Intel 136761602367 — success.
  These include native delivery/keyboard/configuration checks and installed bundle
  doctor, reconciliation and external-model reuse fixtures. Windows passed the
  new async-read regression and synchronized outcome fixture.
- [Main Build binaries](https://github.com/cascad/localvox-light/actions/runs/44876277279)
  and [main Interface](https://github.com/cascad/localvox-light/actions/runs/44876277354)
  — success on the same candidate. The earlier 0.2.3 main failure remains historical.
- Exactly 14 public assets: five product archives, their five SHA-256 files, and
  four installer scripts. No browser trace. Each archive's public checksum matches
  its GitHub asset digest; all four actual latest/download scripts match their
  tagged source and asset digests. Public install.sh passed bash -n.
- In a real Windows ConPTY, the exact published README command was executed:
  `irm https://github.com/cascad/localvox-light/releases/latest/download/install.ps1 | iex`.
  HOME/USERPROFILE/APPDATA/LOCALAPPDATA/TEMP were private paths under target/review
  on D:, inherited LOCALVOX settings were removed, and LLM discovery was directed
  to an unavailable private endpoint. It downloaded the full 116.3 MiB public ZIP
  in 6 s, verified SHA-256, unpacked and reached the real native wizard. The staged
  bundle manifest and native --version/--bundle-check identified v0.2.4. The model
  manifest retained the official alphacephei Vosk source. On the first screen Esc
  produced exit 0 and «Отменено». Assertions confirmed no destination installation,
  .localvox, configuration/PATH/model writes or remaining owned bootstrap staging.

Private driver, transcript and copied public manifests are ignored under
target/review/release-v0.2.4. This public one-liner success supersedes the failed
300 s v0.2.3 public smoke; the corrected-local-source run is kept as a separate
pre-publication measurement. Neither the observed transfer time nor green fixtures
are a network SLA, full dependency/model installation or physical audio acceptance.

The optional cleanup of the interrupted earlier private fixture staging was
rejected by automatic command review (`blocked by policy`). It was left on D:;
no alternate delete mechanism was used. This does not affect delivered assets.
