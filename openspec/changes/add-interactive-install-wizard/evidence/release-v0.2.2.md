# Release v0.2.2 — preparation, 2026-11-20

Owner requested the version bump/release after pushing `5c37e93`. Publication is
pending; this document separates preparation from the eventual hosted release.

## Preparation

- Workspace and desktop application version are **0.2.2**; UI package/lock metadata
  is also **0.2.2**. Cargo.lock changes only the 16 workspace package versions.
  Tagged packaging now rejects a tag that differs from the compiled workspace
  version, so the release tag and CLI version cannot silently diverge again.
- On `5c37e93`, [Interface 44800931317](https://github.com/cascad/localvox-light/actions/runs/44800931317)
  passed 81 browser e2e tests, then failed in setup-mpv.mjs: metadata HTTP **403**.
  The request had no authentication. The response reason/body was not retained;
  a specific rate-limit cause is not asserted from the status alone.
- Node's metadata request now supports GH_TOKEN. The workflow supplies its scoped
  read token in a dedicated mpv step, after npm install/build/tests. The token is
  not supplied to the asset host. SHA-256 verification remains required.
- Local tools delivery: **4 passed**, including actual Node execution with a mock
  fetch and tiny ZIP extracted by native tar, authenticated/public metadata cases,
  no Authorization header on the asset request, expected player/license payload.
- Local release-version guard regression passed. UI production build passed.
  Native installer compiled and reported **0.2.2**. Caches/fixtures stayed on D:;
  no system installation, models, archive or running daemon was changed.
- actionlint, strict specs/change, check-openspec and diff checks passed.

## Pending delivery

### Follow-up: Mac terminal fixture and publication gate

On `98684da`, Interface run `44802653117` passed completely. In Build
`44802653137`, both Linux targets passed, while Mac ARM job `136518605795`
failed in preview navigation: after Stay, the fixture waited for the old completed
menu's text still present in terminal history, sent arrows before a new menu was
active, and reached Summary source instead of Cancel. This is a confirmed stale
screen synchronization error in the fixture, not evidence of a new mpv failure.

All five navigation-menu waits now require the active prompt marker, using the
fixture's existing `active` helper. Assertions/deadlines remain unchanged. The
complete native preview suite passed **11/11** locally after this correction.
The release workflow now reuses the full Interface workflow and gates publication
on both Interface and all five platform builds. A tag may start validation; a
public release is created only after those checks pass. This avoids publishing
with a red/unfinished Interface run.

New main CI, tag creation/push, all five tagged platform builds, full public release
and public bootstrap verification remain **not-run** at preparation time. Prior
green jobs are not claimed as evidence for the new version commit. Hardware/Mac
installation acceptance is still separate from automated delivery.

## Tagged candidate result

Tag v0.2.2 was sent on `f94803e`. Release run `44807211244` stopped at Mac ARM
Delivery job `136534374651`: Ctrl+C was echoed at 12%, but asynchronous curl
continued to 100% before the deliberate digest failure. This is a real Unix
cancellation defect in the new meter; the assertion was preserved. Publication
is gated and did not run. The tag is retained as failed-candidate history.
The correction/next intended release is [v0.2.3](release-v0.2.3.md).
