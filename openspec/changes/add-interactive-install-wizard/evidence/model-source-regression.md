# Model download regression — 2026-11-19

Scope: SETUP-001, INV-SETUP-001, SETUP-006/009 (delta). Reported during an actual
Windows installation of v0.2.1 into `F:\lv`; that installation was not modified.

## Reproduction and correction

- Manifest Vosk mirror
  `https://huggingface.co/mychen76/vosk-models/resolve/main/ru/vosk-model-ru-0.42.zip`
  returned **HTTP 401**, `Invalid username or password`, without credentials.
  The official `https://alphacephei.com/vosk/models/vosk-model-ru-0.42.zip`
  returned **200**, Content-Length **1937602113**, matching the manifest. The
  manifest now uses this official source. SHA-256 remains explicitly unavailable;
  ZIP/marker checks were preserved, not described as authentication.
- Actual PS5 and PS7 subprocesses with console code page 866 reproduced invalid
  UTF-8/Cyrillic. The model script now explicitly emits UTF-8 and uses basic HTTP
  parsing. HTTP failures name their source/status and remove incomplete `.part`.
- Running PS5 from pwsh inherited incompatible PS7 modules: Get-FileHash was
  unavailable and Expand-Archive called an unsupported .NET overload. The native
  script launcher removes inherited PSModulePath so PS5 resolves its own modules.
  This fixes the process boundary instead of replacing individual cmdlets.
- Earlier localhost fixtures validated delivery mechanics, not public model URLs.
  `check-model-sources.py` now checks HEAD + a bounded 64-byte Range GET for every
  downloadable item. A separate CI job gates builds, including tag publication.

## Executed checks

- Source gate: **9/9 public endpoints passed** HEAD and bounded GET, including
  Vosk ZIP signature and advertised size. No complete weights were downloaded.
- Source gate localhost tests: **5 passed** (redirect, auth rejection at HEAD or
  GET, wrong size, login HTML); errors close their HTTP response handles.
- Model storage delivery: **12 passed / 3 skipped**. Real Windows PowerShell 5
  downloads/hash/extract/repeat, Git Bash downloads, OEM UTF-8 checks on PS5/PS7,
  HTTP failure preserving completed models followed by successful retry. Skips:
  disabled 8.3 aliases, no explicit bundled resolver, native Unix symlink test.
- `cargo test --locked -p localvox-light --test setup --target
  x86_64-pc-windows-msvc --jobs 2 -- --test-threads=2`: **11 passed**, including
  real PS5 hash/ZIP round-trip through the production script launcher.
- PackageContracts: **10 passed / 1 skipped** (no explicit full native archive).
  CI delivery contracts: **4 passed / 1 skipped** (same native archive boundary).
- Strict main specs **15/15**, strict change validation, check-openspec and scoped
  rustfmt check passed. Workspace-wide fmt reports pre-existing differences in
  untouched files; they were not reformatted as part of this installation fix.

All fixture data/caches are under repository `target` on D:. The real daemon,
archive, system tools and user's `F:\lv` were untouched. This is local evidence;
new hosted builds/public release, full model installation and hardware acceptance
are **not-run** for this correction.
