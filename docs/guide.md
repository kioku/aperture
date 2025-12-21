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

# List commands for an API
aperture list-commands my-api

# Get detailed command information
aperture api my-api --describe-json
```

### 3. Execute Commands

```bash
# Flag-based syntax (default)
aperture api my-api users list
aperture api my-api users get-user-by-id --id 123
aperture api my-api users create --name "John Doe" --email "john@example.com"
```

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

# Request body (JSON)
aperture api my-api users create --body '{"name": "John", "email": "john@example.com"}'

# Multiple parameters
aperture api my-api orders search --status pending --created-after 2024-01-01
```

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

## Search Commands

Find operations across all registered APIs:

```bash
# Search by keyword
aperture search "create user"

# Search within specific API
aperture search "list" --api my-api

# Regex search
aperture search "get.*by.*id" --verbose
```

## Shortcuts

Execute operations using shorthand:

```bash
# By operation ID
aperture exec getUserById --id 123

# By HTTP method and path
aperture exec GET /users/123

# By tag and operation
aperture exec users list
```

## API Exploration

### Overview

Get a quick summary of an API with statistics and example commands:

```bash
# Overview of a specific API
aperture overview my-api

# Overview of all registered APIs
aperture overview --all
```

### Interactive Documentation

Browse detailed documentation for APIs and operations:

```bash
# Interactive help menu
aperture docs

# API overview
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
aperture config reinit
```
