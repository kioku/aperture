# CLI Naming Conventions

This document defines Aperture's canonical naming scheme for commands, subcommands, and examples.

## Canonical naming rules

### Top-level commands

Use clear, full-word command names for the primary CLI surface:

- `api`
- `commands`
- `run`
- `search`
- `docs`
- `overview`
- `config`

### `config` subcommands

Use `verb-resource` form for management operations:

- `add`, `list`, `remove`, `edit`
- `set-url`, `get-url`, `list-urls`
- `set-secret`, `list-secrets`, `remove-secret`, `clear-secrets`
- `set-mapping`, `list-mappings`, `remove-mapping`

Singular resource names are used for single-target actions (`set-url`, `remove-secret`), while plural is used for collection-oriented operations (`list-urls`, `clear-secrets`, `list-mappings`).

### Examples and help text

All examples in help text and documentation should use canonical command names.

## Compatibility aliases and migration

To preserve backward compatibility with existing scripts, the CLI supports legacy aliases:

- `list-commands` (legacy) → `commands` (canonical)
- `exec` (legacy) → `run` (canonical)

Migration guidance:

1. Use canonical names in all new scripts and docs.
2. Existing scripts using aliases remain functional.
3. Prefer canonical names when updating existing automation.
