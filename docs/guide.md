# User Guide

This guide covers day-to-day usage of Aperture for interacting with APIs.

## Basic Workflow

### 1. Register an API

```bash
# From a local file
aperture config add my-api ./openapi.yaml

# From a URL
aperture config add my-api https://api.example.com/openapi.yaml
```

### 2. Explore Available Commands

```bash
# List registered APIs
aperture config list

# Land in an API context (overview + next actions)
aperture api my-api

# List commands for an API
aperture commands my-api

# Get detailed command information (machine-oriented)
aperture api my-api --describe-json
```

### Structured Discovery Output

For script-friendly discovery data, use structured output modes:

```bash
aperture commands my-api --format json
aperture overview my-api --format json
aperture docs my-api --format json
aperture docs my-api users get-user-by-id --format json
aperture config api list --json
```

Top-level response shapes are stable for scripting:

- `commands --format json` → `{ api, groups[] }`
- `overview <api> --format json` → `{ api, statistics, quick_start, sample_operations[] }`
- `overview --all --format json` → `{ apis[] }`
- `docs --format json` → `{ mode: "interactive", apis[] }`
- `docs <api> --format json` → `{ mode: "api-reference", api, categories[], example_paths[] }`
- `docs <api> <tag> <operation> --format json` → `{ mode: "operation", api, operation }`
- `config api list --json` → `[{ name, ... }]` (`--verbose` adds endpoint details)

### 3. Execute Commands

```bash
# Flag-based syntax (default)
aperture api my-api users list
aperture api my-api users get-user-by-id --id 123
aperture api my-api users create --name "John Doe" --email "john@example.com"
```

## CLI Naming Conventions

Aperture follows these naming rules across commands, subcommands, and examples:

- **Top-level commands:** prefer clear, full words (`commands`, `run`, `search`, `docs`, `overview`).
- **Config subcommands:** use `verb-resource` (`set-url`, `get-url`, `list-secrets`).
- **Examples and docs:** always use canonical command names.

### Compatibility aliases

To avoid breaking existing scripts, Aperture supports these legacy aliases:

- `aperture list-commands` → `aperture commands`
- `aperture exec` → `aperture run`

These aliases remain available for compatibility, but new usage should prefer canonical names.

## Command Syntax

Aperture uses flag-based syntax for all parameters:

```bash
aperture api <api-name> <tag> <operation> [--param value ...]
```

**Examples:**

```bash
# Path parameters
aperture api my-api users get-user-by-id --id 123

# Query parameters
aperture api my-api users list --limit 10 --offset 0

# Request body (inline JSON)
aperture api my-api users create --body '{"name": "John", "email": "john@example.com"}'

# Request body from a file (avoids shell-quoting issues with large payloads)
aperture api my-api users create --body-file ./payload.json

# Request body from stdin
echo '{"name": "John"}' | aperture api my-api users create --body-file -

# Multiple parameters
aperture api my-api orders search --status pending --created-after 2024-01-01
```

### Flag Scoping Model

Execution-oriented flags are scoped to execution commands (`api`, `run`) instead of being global.

```bash
# ✅ Scoped execution flags on execution commands
aperture api my-api --dry-run users get-user-by-id --id 123
aperture run --dry-run getUserById --id 123

# ❌ Rejected on non-execution commands
aperture docs --dry-run
```

Universal flags remain available everywhere: `--json-errors`, `--quiet`, and `-v`.

### Legacy Positional Syntax

For backwards compatibility, positional arguments are available:

```bash
# Enable with --positional-args
aperture api my-api --positional-args users get-user-by-id 123
```

## Output Formats

### JSON (Default)

```bash
aperture api my-api users get-user-by-id --id 123
```

### YAML

```bash
aperture api my-api users get-user-by-id --id 123 --format yaml
```

### Table

```bash
aperture api my-api users list --format table
```

## Response Filtering

Use the `--jq` flag to extract specific fields from responses.

### Basic Filtering (Always Available)

```bash
# Single field
aperture api my-api users get-user-by-id --id 123 --jq '.name'

# Nested field
aperture api my-api users get-user-by-id --id 123 --jq '.address.city'

# Array index
aperture api my-api users list --jq '.users[0]'
```

### Advanced Filtering (Requires `--features jq`)

Build with JQ support for advanced queries:

```bash
cargo install aperture-cli --features jq
```

Then use full JQ syntax:

```bash
# Filter array elements
aperture api my-api users list --jq '[.users[] | select(.active == true)]'

# Transform output
aperture api my-api users list --jq '.users | map({id, name})'

# Count results
aperture api my-api users list --jq '.users | length'
```

## Response Caching

Cache responses to reduce redundant API calls:

```bash
# Enable caching with default TTL (300 seconds)
aperture api my-api --cache users list

# Custom TTL (in seconds)
aperture api my-api --cache --cache-ttl 600 users list

# Disable caching explicitly
aperture api my-api --no-cache users list
```

### Cache Management

```bash
# View cache statistics
aperture config cache-stats my-api

# Clear cache for an API
aperture config clear-cache my-api
```

## Command Mapping

Customize the CLI command tree without modifying the OpenAPI spec. Rename groups, rename operations, add aliases, or hide commands.

### Rename a Tag Group

```bash
# Rename "User Management" to "users"
aperture config set-mapping my-api --group "User Management" users
aperture config reinit my-api
```

### Rename an Operation

```bash
# Rename getUserById to "fetch"
aperture config set-mapping my-api --operation getUserById --name fetch
aperture config reinit my-api

# Now use the new name
aperture api my-api users fetch --id 123
```

### Add Aliases

```bash
# Add "get" and "show" as aliases for an operation
aperture config set-mapping my-api --operation getUserById --alias get
aperture config set-mapping my-api --operation getUserById --alias show
aperture config reinit my-api

# All three work
aperture api my-api users get-user-by-id --id 123
aperture api my-api users get --id 123
aperture api my-api users show --id 123
```

### Move an Operation to a Different Group

```bash
# Move getUserById from its original tag group to "accounts"
aperture config set-mapping my-api --operation getUserById --op-group accounts
aperture config reinit my-api
```

### Hide an Operation

```bash
# Hide a deprecated or internal operation from help output
aperture config set-mapping my-api --operation deleteUser --hidden
aperture config reinit my-api

# The command still works but doesn't appear in --help
aperture api my-api users delete-user --id 123
```

### View and Remove Mappings

```bash
# List all mappings
aperture config list-mappings my-api

# Remove an operation mapping
aperture config remove-mapping my-api --operation getUserById

# Remove a group mapping
aperture config remove-mapping my-api --group "User Management"

# Apply changes
aperture config reinit my-api
```

> **Note:** All mapping changes require `aperture config reinit` to take effect. The mappings are applied during cache generation.

## Search Commands

**Canonical role:** primary intent-first discovery. Use search when you know what you want to do but not where the command is.

Search matches operation names, descriptions, display names, and aliases from command mappings.

```bash
# Search by keyword
aperture search "create user"

# Search within specific API
aperture search "list" --api my-api

# Regex search
aperture search "get.*by.*id" --verbose

# Finds operations by display name or alias
aperture search "fetch"
```

## Shortcuts

Execute operations using shorthand:

```bash
# By operation ID
aperture run getUserById --id 123

# By HTTP method and path
aperture run GET /users/123

# By tag and operation
aperture run users list
```

## API Exploration

Use this human workflow for discovery:

1. **Land** with `api <context>`
2. **Find** with `search`
3. **Inspect** with `docs`
4. **Execute** with `api <context> <tag> <operation>`

### Overview

**Canonical role:** orientation. Get a high-level API summary with statistics and starter paths.

```bash
# Overview of a specific API
aperture overview my-api

# Overview of all registered APIs
aperture overview --all
```

### Commands Tree

**Canonical role:** structural lookup. Get a terse tree of available command paths.

```bash
# Canonical command name
aperture commands my-api

# Legacy alias
aperture list-commands my-api
```

### Interactive Documentation

**Canonical role:** deep reference. Inspect exact operation usage, parameters, request bodies, and responses.

```bash
# Interactive help menu
aperture docs

# API reference index
aperture docs my-api

# Detailed command help with parameters and examples
aperture docs my-api users get-user

# Enhanced formatting with tips
aperture docs my-api users get-user --enhanced
```

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Failure (API error, network error, validation error) |

Use exit codes in scripts:

```bash
if aperture api my-api users get-user-by-id --id 123; then
    echo "User found"
else
    echo "Request failed"
fi
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `APERTURE_BASE_URL` | Global base URL override |
| `APERTURE_ENV` | Environment selector (e.g., `staging`, `prod`) |
| `RUST_LOG` | Log level (`debug`, `info`, `warn`, `error`) |

## Common Patterns

### Piping to Other Tools

```bash
# Pretty print with jq
aperture api my-api users list | jq .

# Save to file
aperture api my-api users list > users.json

# Process with other tools
aperture api my-api users list --jq '.users[].email' | sort | uniq
```

### Scripting

```bash
#!/bin/bash
set -e

# Fetch and process users
USERS=$(aperture api my-api --json-errors users list --jq '.users')
COUNT=$(echo "$USERS" | jq 'length')

echo "Found $COUNT users"

# Iterate over results
echo "$USERS" | jq -c '.[]' | while read -r user; do
    ID=$(echo "$user" | jq -r '.id')
    NAME=$(echo "$user" | jq -r '.name')
    echo "Processing user $ID: $NAME"
done
```

### CI/CD Integration

```yaml
# GitHub Actions example
- name: Fetch deployment status
  run: |
    aperture api deploy-api --json-errors status get --env production
  env:
    DEPLOY_API_TOKEN: ${{ secrets.DEPLOY_API_TOKEN }}
```

## Troubleshooting

### Debug Mode

```bash
RUST_LOG=debug aperture api my-api users list
```

### Dry Run

Preview requests without executing:

```bash
aperture api my-api --dry-run users create --name "Test"
```

### Validate Spec

Re-add with strict mode to check for issues:

```bash
aperture config add --strict my-api ./openapi.yaml
```

### Clear and Reinitialize

```bash
# Clear cache for specific API
aperture config clear-cache my-api

# Reinitialize all cached specs
aperture config reinit --all

# Reinitialize specific spec
aperture config reinit my-api
```
