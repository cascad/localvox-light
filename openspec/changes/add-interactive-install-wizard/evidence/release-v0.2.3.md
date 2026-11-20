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
