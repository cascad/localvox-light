# Initial bundle download progress — 2026-11-19

Owner requested visible download progress before the native wizard. SETUP-009
delta now specifies a terminal meter, unknown-size indication and plain logs.

## Implementation

- Windows bootstrap streams HTTP into its existing private `.part` using a 64 KiB
  buffer. Response waits/reads repaint progress, with a 60-second no-response
  deadline; Content-Length comes from the GET, including redirects. Resources are
  disposed before staging cleanup. Prior console encoding is restored afterwards.
- Unix bootstrap uses one curl GET with separate header/output files and bounded
  progress sampling, without parsing curl's human output. File size uses native
  stat (GNU/BSD); final-response headers supply the optional total. EXIT cleanup
  stops/waits for only its owned curl before removing the private staging.
- TTY: branch, compact bar/percentage, downloaded/total MiB, elapsed time. Unknown
  size: rotating indicator plus downloaded MiB. Redirected logs: ordinary lines,
  no ANSI/carriage-return redraw. NO_COLOR is respected for the meter's color.
- SHA-256 and unpacking remain separate visible phases after transfer. A completed
  meter means only transport completion, not successful integrity/installation.

## Executed evidence

- Bootstrap delivery **6 passed / 1 skipped** on Windows. All four Unix archive
  routes tested through Git Bash. Actual PS5 and PS7 bootstrap downloads exercised
  slow known/unknown size, HTTP 503, truncated connection, digest mismatch and
  refusal to launch the native installer on failure. Windows fresh/reuse/Check
  and unsafe-archive rejection remained green.
- A small portable-pty example drives real ConPTY/PTY, not redirected stdin.
  Both Windows PowerShell 5 and Git Bash passed known/unknown-size continuous
  redraw and Ctrl+C during transfer; no installer launch, no remaining staging.
  PowerShell -File itself returns 0 on Ctrl+C; cancellation checks use cleanup,
  absence of transfer completion and absence of native launch.
- Actual ConPTY captures were rendered using the existing xterm/Playwright stand
  and visually inspected locally: `target/review/bootstrap-progress/known.png`
  and `unknown.png` (fixture v9.8.7, not a published release). Logs are private
  fixture paths/URLs, no user models/archive or system programs were used.
- PackageContracts **10 passed / 1 skipped**; CI delivery **4 passed / 1 skipped**.
  Skipped full native archive checks were not counted as successful runs.
- Strict main specs/change, check-openspec, scoped rustfmt and diff checks passed.
  GitHub workflow builds the runner and executes these terminal fixtures on all
  five platform targets before packaging; that new hosted run is **not-run**.

Native macOS/Linux execution and real release transfer for this revision remain
**not-run** locally; Git Bash target simulation does not replace those checks.
Public release assets have not been changed or published by this work.
