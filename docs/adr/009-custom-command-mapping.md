# ADR 009: Custom Command Structure Mapping

## Status

Accepted

## Context

Aperture generates CLI commands from OpenAPI specs using a deterministic algorithm:
- The first tag becomes the command group (kebab-cased)
- The `operationId` becomes the subcommand name (kebab-cased)

This works well for specs with clean naming, but real-world specs frequently produce
awkward command trees: verbose operation IDs, inconsistent tag naming, redundant
prefixes, or unwanted deprecated endpoints cluttering the CLI.

Users had no way to customize the command tree without editing the OpenAPI spec itself,
which is not viable for third-party or frequently-updated specifications.

## Decision

We implement a **config-based external mapping** (Option B from issue #73) as the
first increment of a hybrid approach. This adds a `command_mapping` section to the
per-API configuration in `config.toml`:

```toml
[api_configs.sentry.command_mapping]

[api_configs.sentry.command_mapping.groups]
"User Management" = "users"
"Organization Settings" = "orgs"

[api_configs.sentry.command_mapping.operations.getUserById]
name = "fetch"
group = "accounts"
aliases = ["get", "show"]

[api_configs.sentry.command_mapping.operations.deleteUser]
hidden = true
```

### Precedence Model

Mappings are applied during cache generation (`config add` / `config reinit`):

```
Config mapping  >  Default (tag/operationId)
```

A future increment can add `x-aperture-cli` OpenAPI extensions (Option A), forming
the full hybrid (Option D) with three-level precedence:

```
Config mapping  >  x-aperture-cli extension  >  Default
```

### Why Config-Based First

1. **Works with third-party specs** — no spec modification required
2. **Follows existing patterns** — mirrors `secrets` and `base_url_override` in `ApiConfig`
3. **Independently useful** — covers the most common use case
4. **Incrementally extensible** — extension support can be layered on later

### Collision Detection

During cache generation, the mapping application validates that:
- No two operations resolve to the same `(group, name)` pair
- No alias collides with another operation's name or alias within the same group
- No customized group name conflicts with built-in commands (`config`, `search`, `exec`, `docs`, `overview`)

Collisions produce hard errors that prevent cache generation.

### Stale Mapping Handling

When a spec is updated and operations change:
- Mappings referencing non-existent tags or operation IDs produce **warnings**, not errors
- The spec is still processed successfully with stale mappings ignored
- Users can clean up stale mappings via `config remove-mapping`

## Consequences

### Positive

- Users can customize command trees for any spec without modification
- The `--describe-json` manifest reflects customized names for agent consumption
- Search and shortcut resolution recognize display names and aliases
- Cache format versioning (v4 → v5) ensures clean migration

### Negative

- Mappings are decoupled from the spec and can drift when specs are updated
- Users must run `config reinit` after changing mappings
- The config surface area grows (new `command_mapping` section, 3 new CLI subcommands)

### Neutral

- Cache format version bump triggers automatic reinit for existing specs
- All new `CachedCommand` fields use `#[serde(default)]` for backward compatibility

## References

- Issue #73: Custom command structure mapping for API specifications
- Architecture SDD §5.1: Command Generation Strategy
- Existing pattern: `x-aperture-secret` + `config set-secret` override precedence
