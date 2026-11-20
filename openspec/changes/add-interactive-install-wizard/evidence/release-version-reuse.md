# Version-aware bootstrap reuse — 2026-11-20

Owner explicitly changed ordinary retry: an existing binary must not suppress a
new published release. This extends SETUP-009; prior `--update`-only behavior in
the historical hosted-575aca2 report is superseded by this implementation.

## Implementation

- `package-release.py --release-tag` records the real release tag in bundle.json.
  Cargo's workspace version (currently 0.1.0) is not used as release identity.
  Main development artifacts have null identity. Tag/manual-release workflows
  explicitly pass their release tag; tagged packaging smoke verifies it through
  the staged native installer after the ordinary executable checks.
- Native `--bundle-check --release-tag` first verifies platform and every file,
  then returns 0 for the same tag, 3 for an installed newer semver release, or 2
  for another/unknown release or corruption. Semver comparison uses the existing
  locked semver 1.0.27 crate, not lexical version comparison or a shell parser.
  The original bundle-check without a release tag retains its 0/2 behavior.
- Both bootstrap scripts resolve latest before considering reuse. Matching intact
  installations skip the archive, but still open the wizard. Older/unknown/damaged
  bundles download the pinned offered release through the existing SHA/staging path.
  Legacy installers do not support the new flag and therefore upgrade once.
- Latest never automatically downgrades a newer installed semver release. Explicit
  `--tag` / `-Tag` and forced Update permit the chosen replacement. Update still
  forces an archive fetch, even for the same tag.
- Latest metadata failure aborts instead of silently continuing an old installer.
  Metadata requests have bounded timeouts. Check returns before remote lookup and
  still uses the local installation. Configuration/archive/model preservation
  remains the native publication/planning responsibility.

## Executed checks

- Native install suite: **12 passed**, including actual installer subprocesses
  checking packaged same/older/newer/prerelease identities and corrupt files.
  Existing preservation, staging guards and real terminal wizard tests passed.
  The release test was also run with `--locked` after the lockfile update.
- Bootstrap localhost suite: **8 passed / 1 skipped**. PS5 and Git Bash/ConPTY
  exercised old → new → same without another archive, missing legacy identity,
  damaged bundle, failed latest lookup without native launch, offline Check,
  retaining a newer installation, and explicit pinned downgrade. Existing PS7
  transfer/error/plain-log and progress/cancellation checks remained green.
- PackageContracts: **11 passed / 1 skipped**, including release identity in the
  manifest and development artifacts without a published identity.
- actionlint, strict main specs/change, scoped rustfmt and check-openspec passed.
  Fixture data and compiler/dependency caches stayed under repository target on D:.

The skipped scenarios are native Unix acceptance on Windows and an optional full
native release archive that was not supplied. No fresh full product archive or
new hosted run/public release is claimed. User installations/models/daemon were
not modified. Native Mac/Linux acceptance remains separate from Git Bash routes.
