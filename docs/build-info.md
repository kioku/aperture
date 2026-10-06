# Build identity

`aperture --version` (or `-V`) reports the package version followed by the
seven-character source revision and state, for example
`aperture-cli 0.1.9 (bb5ef54, clean)`. Archives without source metadata report
`(unknown, unknown)`. Use `aperture build-info`
or `aperture build-info --json` for the embedded version, full source revision,
and `source_state` (`clean`, `dirty`, or `unknown`). This command does not load
configuration or contact services. Metadata contains no time, host path, or
credentials.

Ordinary Cargo builds detect Git HEAD and tracked/untracked changes. Without Git
or an available commit, the revision and state are `unknown`; this never means
main. A dirty revision names the base commit, not a hash of the local changes.
Different clean commits at the same package version report different revisions.
Dirty edits on the same base are not uniquely identified.

Builders of source archives can supply both environment variables:

```sh
APERTURE_BUILD_REVISION=ce7082d0003cb0b35ae9ed19c5b9a79b1dc75a6f \
APERTURE_BUILD_SOURCE_STATE=clean cargo build --release
```

Revision must be a full 40- or 64-character hexadecimal Git object ID, or
`unknown`. State must be `clean`, `dirty`, or `unknown`. An unknown revision
requires unknown state. Missing pairs or invalid values fail the build rather
than embedding misleading metadata. Builder claims are not cryptographically
verified against archive contents; builders must supply the actual source ID.
Nix supplies `self.rev` or the revision portion of `self.dirtyRev` before cleaning
source; non-Git flakes explicitly report unknown. Release binaries supply the
checked-out Git revision, including cross builds.
