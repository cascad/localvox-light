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

New main CI, tag creation/push, all five tagged platform builds, full public release
and public bootstrap verification remain **not-run** at preparation time. Prior
green jobs are not claimed as evidence for the new version commit. Hardware/Mac
installation acceptance is still separate from automated delivery.
