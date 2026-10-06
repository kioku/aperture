# Aperture 0.2.0 upgrade notes

This release keeps the major version at zero and introduces a new Rust crate
compatibility boundary. Existing `0.1` Cargo requirements do not automatically
select `0.2.0`; Rust consumers should explicitly update their dependency and
adapt public struct literals and enum patterns before upgrading.

## Compatibility changes

- Public invocation and CLI types have changed since 0.1.9. Review
  `OperationCall`, `RequestBody`, `ExecutionContext`, `ExecutionResult`, and the
  CLI's `ExecutionFlags` and `Commands` definitions. Prefer
  `..ExecutionContext::default()` for optional execution settings.
- Shortcut-resolution information now goes to stderr. Successful `run`/`exec`
  stdout contains only requested response data; consumers parsing the old
  informational prefix should remove that workaround.
- `--version` and `-V` append embedded revision and source state. Use
  `build-info --json` for structured identity instead of parsing the full
  human-readable version string.

## Highlights

The release adds ranked intent discovery, reusable shortcut indexes, structured
search (`search --format json`), embedded build identity, bundled and installed
skills, and clearer authentication/spec-fetch workflows. It includes security
repairs for redirects, diagnostic redaction, captured header interpolation,
cache identity, response/cache bounds, and checked retry durations.

Existing default text search remains available. Execution can be embedded through
`aperture_cli::invocation` and `aperture_cli::engine::executor`; this is a public
Rust library, not a promise of a frozen SDK API.

## Distribution and rollout

GitHub release binaries use default Cargo features. Installations needing jq or
OpenAPI 3.1 must retain the corresponding Cargo features or Nix package variant
(`aperture-full` enables both). Publishing a release does not update existing Nix
profiles, Foundry infrastructure pins, or executor images; those are separate
rollouts.
