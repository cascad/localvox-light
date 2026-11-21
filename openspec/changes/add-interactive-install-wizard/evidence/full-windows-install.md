# Full Windows installation after owner failure — 2026-11-21

SETUP-001/003/009 (delta), INV-SETUP-001. The public 0.2.4 bootstrap smoke proved
only download/wizard/cancel. It did not prove installation of all selected models;
the owner's actual embedding checksum failure invalidated the earlier broad
readiness claim. This report records the subsequent real local installation.

## Verified model replacement

The official Sherpa ONNX speaker-model release asset 766772283 was replaced on
2026-11-19. GitHub reports size 39593766 and SHA-256
78d4730ad5a6dc1ed86914e2d76c796c6bfc6852f558dd1cbb9b16abd3c3f672.
A complete official download reproduced that digest. The previous local file
still matches historical e2d2048292e055f7b61cdec3db010503f35369b245bf0b3bbad021c9a91e4053.
Comparison proved the change is producer_version `2.4.1` -> `2.14.1`; all 39593750
bytes after that metadata field are identical, including graph and weights.
The old golden provenance is retained. Real ONNX inference with the new file
passed: 512 dimensions, cosine 1.00000000; fbank MAE 1.403e-6, maximum 3.328e-4.
The first attempted run used a wrong environment key and skipped inference;
it was rejected as evidence and rerun with LOCALVOX_DIARIZE_MODEL_DIR, requiring
the explicit cosine marker. SHA-256 enforcement is unchanged.

The source gate now also compares the published GitHub asset digest with the
manifest before HEAD/Range checks or builds. Its token is confined to API metadata,
never model requests. Offline stale/missing digest and token-isolation tests pass;
the actual live gate passed 9/9 sources. This metadata check is not described as
computing full weight hashes in CI.

## Actual installation and repeat in F:\lv

An official package-release.stage candidate used the installed release as its base,
locally built installer/daemon and corrected scripts. Its release_tag is null:
these local changes are not misrepresented as a published 0.2.4 artifact. Original
program/config backups and private transcripts are ignored under
target/review/full-install-2026-11-21 on D:. No user archive sessions or running
daemon were used for the check.

The real native installer was driven through ConPTY and confirmed with the saved
five optional selections: speakers, summary, links, player and names. Vosk/GigaAM
and segmentation were already downloaded by the owner and reused without mtime
changes. Embedding and GLiNER, tokenizer/config and license files were really
downloaded and installed. All eight manifest file hashes were independently
recomputed against F:\lv/models, including the 579717643-byte GLiNER file. Vosk's
am/final.mdl marker is present; no unsupported cryptographic claim is made for its
unhashed upstream ZIP. Ollama and qwen3.5:9b already existed and were reused.

The installer exited 0, removed installation.pending.json and published active
settings. Native localvox-install --check returned 0: Vosk, GigaAM, worker,
diarization, GLiNER, archive, Ollama CLI/API and selected LLM all OK. The corrected
optimized installer/daemon were subsequently installed with ONNX enabled. A repeat
from the actual installed F:\lv\localvox-install.exe finished successfully with
no downloads: every model size/mtime and the active .env bytes were unchanged.

## Presentation and fixture corrections

- A standard indicatif spinner owns the action line and elapsed time. Child stdout
  and stderr retain separate bounded UTF-8 line buffers and are printed above it;
  completion clears the renderer before other installer blocks write. No custom
  cursor bookkeeping or repeated minute heartbeat lines remain. Errors retain the
  stage, child diagnostic and unsuccessful completion. Redirected output is plain.
- Per-component fetches omit other components. An actual --skip/required-only
  exclusion says «Не выбрано». Real PS5 and Bash syntax checks plus model storage
  fixtures cover the distinction; native Unix execution without jq on this host
  is not claimed.
- The first full fixture incorrectly passed a verbatim \\?\ TEMP path, which
  PyInstaller yt-dlp rejected with «Failed to create parent directory structure».
  That fixture problem led to a spurious winget no-upgrade refusal, not a proven
  product timeout. The fixture now uses ordinary absolute paths. Fresh PATH also
  motivated discovery of registered portable yt-dlp via Windows Uninstall metadata;
  a private-registry test checks exact package IDs and absolute roots. The bounded
  yt-dlp probe allows its packaged Python startup instead of a 3 s near-boundary
  budget. No installed yt-dlp binary was replaced.
- A completed first install was initially reported failed by the driver because it
  stopped reading ConPTY at process exit before the final frame arrived. Pending
  state, settings and doctor 0 proved completion. The driver now drains the final
  output and subsequent confirmed repeat runs capture the completion message.
- Broader ONNX-enabled tests exposed two queue-discovery fixtures fingerprinting
  real models and missing their unchanged 2 s lock-isolation deadline. They now
  hold the existing environment guard and select absent private model paths.
  No production queue behavior or assertion/deadline was weakened; the full
  ONNX library run then passed 18 tests / 12 explicit helpers/fixtures ignored.

Executed related checks: native install 12/12, setup 11/11, setup library 3 passed
/ 1 subprocess helper ignored, installer preview 11/11, source gate 8/8, Windows
model-storage fixtures 10 passed / 6 unavailable-platform/resolver cases skipped.
Actual speaker inference ran, rather than silently passing its no-model branch.

## Boundaries

This proves a real selected installation with reuse on this Windows host, not a
clean OS, fresh Ollama install, physical capture or native Mac/Linux acceptance.
The public 0.2.4 catalog is still old. The owner's next request explicitly requires
v0.2.5 publication and a separate clean public install with real downloads before
being notified that GitHub installation is ready. That gate remains pending.

Follow-up: [published v0.2.5 and the clean public Windows installation](release-v0.2.5.md)
subsequently passed this gate. The paragraph above records the local preparation
boundary; it is not the current publication status.
