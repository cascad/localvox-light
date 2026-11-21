# Release v0.2.4 — preparation, 2026-11-21

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

Native hosted checks and public v0.2.4 delivery remain pending. Hardware acceptance
and installation of real OS packages/models remain not-run.

Strict main specs (15/15), strict change, check-openspec.py (15 capabilities,
88 requirements, 188 scenarios), scoped rustfmt, actionlint, metadata/package
version guard and diff checks passed before commit. No existing tag is rewritten.
