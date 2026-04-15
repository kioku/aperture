# Configuration Reference

Aperture stores configuration in `~/.config/aperture/`. This document covers all configuration options and commands.

## Config Command Taxonomy

`aperture config` is organized into nested administrative domains:

- `aperture config api ...` — specification lifecycle
- `aperture config url ...` — base URL overrides
- `aperture config secret ...` — auth secret mappings
- `aperture config cache ...` — response cache operations
- `aperture config setting ...` — global settings
- `aperture config mapping ...` — command tree customization

Legacy flat commands (for example `config set-url` and `config settings`) remain supported for compatibility, but new documentation uses nested domain commands.

## Directory Structure

```
~/.config/aperture/
├── specs/                    # Original OpenAPI specification files
│   ├── my-api.yaml
│   └── other-api.json
├── .cache/                   # Pre-processed binary cache
│   ├── my-api.bin
│   ├── other-api.bin
│   ├── .metadata.json        # Cache version tracking
│   └── response_cache/       # HTTP response caches
│       ├── my-api/
│       └── other-api/
└── config.toml               # Global configuration
```

## Specification Management

### Add a Specification

```bash
# From local file
aperture config api add my-api ./openapi.yaml

# From URL
aperture config api add my-api https://api.example.com/openapi.yaml

# With strict validation (reject unsupported features)
aperture config api add --strict my-api ./openapi.yaml
```

### List Specifications

```bash
aperture config api list
```

### Remove a Specification

```bash
aperture config api remove my-api
```

### Reinitialize Cache

Rebuild all cached specifications:

```bash
aperture config api reinit --all
```

## Base URL Management

Override the base URL defined in OpenAPI specs.

### Set Base URL

```bash
# Permanent override
aperture config url set my-api https://api.example.com

# Environment-specific
aperture config url set my-api --env staging https://staging.api.example.com
aperture config url set my-api --env prod https://api.example.com
```

### View URL Configuration

```bash
# Single API
aperture config url get my-api

# All APIs
aperture config url list
```

### URL Resolution Priority

1. **CLI argument** (for testing)
2. **Environment-specific config** (when `APERTURE_ENV` is set)
3. **Per-API override** (set via `config url set`)
4. **`APERTURE_BASE_URL`** environment variable
5. **OpenAPI spec** server URL
6. **Fallback** `https://api.example.com`

### Using Environments

```bash
# Configure environments
aperture config url set my-api --env dev https://dev.api.example.com
aperture config url set my-api --env staging https://staging.api.example.com
aperture config url set my-api --env prod https://api.example.com

# Select environment at runtime
APERTURE_ENV=staging aperture api my-api users list
```

## Server URL Template Variables

OpenAPI specs can define templated server URLs:

```yaml
servers:
  - url: https://{region}.api.example.com/{version}
    variables:
      region:
        default: us
        enum: [us, eu, asia]
        description: API region
      version:
        default: v1
```

### Provide Variables at Runtime

```bash
aperture api my-api users list --server-var region=eu --server-var version=v2
```

### Variable Behavior

- **With default**: Optional, uses default if not provided
- **Without default**: Required, error if not provided
- **With enum**: Validated against allowed values
- **URL encoding**: Values are automatically URL-encoded

## Secret Management

See [Security Model](security.md) for complete documentation.

### Quick Reference

```bash
# Configure secret mapping
aperture config secret set my-api bearerAuth --env API_TOKEN

# Interactive setup
aperture config secret set my-api --interactive

# List configured secrets
aperture config secret list my-api
```

## Cache Management

### Response Cache

```bash
# View cache statistics
aperture config cache stats my-api

# Clear response cache
aperture config cache clear my-api
```

### Specification Cache

```bash
# Rebuild all spec caches
aperture config api reinit --all

# Rebuild specific spec cache
aperture config api reinit my-api

# Remove and re-add specific spec
aperture config api remove my-api
aperture config api add my-api ./openapi.yaml
```

## Command Mapping

Customize the CLI command tree for any API specification without modifying the original OpenAPI spec. This is especially useful for third-party APIs with verbose operation names, inconsistent tagging, or deprecated endpoints.

### How It Works

Command mappings are stored in `config.toml` under `api_configs.<name>.command_mapping` and applied during cache generation (`config api add` or `config api reinit`). Agents see the effective names in `--describe-json`.

### Group Renames

Rename the tag-derived command groups:

```bash
# "User Management" tag becomes "users" group
aperture config mapping set my-api --group "User Management" users

# "Organization Settings" becomes "orgs"
aperture config mapping set my-api --group "Organization Settings" orgs
```

### Operation Mappings

Customize individual operations by their `operationId`:

```bash
# Rename the subcommand
aperture config mapping set my-api --operation getUserById --name fetch

# Move to a different group
aperture config mapping set my-api --operation getUserById --op-group accounts

# Add an alias
aperture config mapping set my-api --operation getUserById --alias get

# Remove an alias
aperture config mapping set my-api --operation getUserById --remove-alias get

# Hide from help output
aperture config mapping set my-api --operation deleteUser --hidden

# Unhide
aperture config mapping set my-api --operation deleteUser --visible
```

### Viewing Mappings

```bash
aperture config mapping list my-api
```

**Output:**

```
Command mappings for API 'my-api':

  Group renames:
    'User Management' → 'users'

  Operation mappings:
    'getUserById':
      name: 'fetch'
      group: 'accounts'
      aliases: ['get', 'show']
    'deleteUser':
      hidden: true
```

### Removing Mappings

```bash
# Remove a group mapping
aperture config mapping remove my-api --group "User Management"

# Remove an operation mapping
aperture config mapping remove my-api --operation getUserById
```

### Applying Changes

Mappings take effect after rebuilding the cache:

```bash
aperture config api reinit my-api
```

### Config File Format

Mappings are stored in `config.toml`:

```toml
[api_configs.my-api.command_mapping]

[api_configs.my-api.command_mapping.groups]
"User Management" = "users"
"Organization Settings" = "orgs"

[api_configs.my-api.command_mapping.operations.getUserById]
name = "fetch"
group = "accounts"
aliases = ["get", "show"]

[api_configs.my-api.command_mapping.operations.deleteUser]
hidden = true
```

### Collision Detection

During cache generation, Aperture validates that:
- No two operations resolve to the same `(group, name)` pair
- No alias collides with another operation's name or alias in the same group
- Customized group names don't conflict with built-in commands (`api`, `commands`, `run`, `config`, `search`, `docs`, `overview`; legacy aliases `list-commands` and `exec` are also reserved)

Collisions produce hard errors that prevent cache generation.

### Stale Mapping Handling

When a spec is updated and operations change:
- Mappings referencing non-existent tags or operation IDs produce **warnings**, not errors
- The spec is still processed with stale mappings ignored
- Clean up stale mappings with `config mapping remove`

## Global Settings Management

Aperture provides commands to view and modify global settings without manually editing the config file.

### List All Settings

```bash
aperture config setting list
```

**Output:**
```
Available configuration settings:

  default_timeout_secs = 30
    Type: integer  Default: 30
    Default timeout for API requests in seconds

  agent_defaults.json_errors = false
    Type: boolean  Default: false
    Output errors as JSON by default

  retry_defaults.max_attempts = 0
    Type: integer  Default: 0
    Maximum retry attempts (0 = disabled)

  retry_defaults.initial_delay_ms = 500
    Type: integer  Default: 500
    Initial delay between retries in milliseconds

  retry_defaults.max_delay_ms = 30000
    Type: integer  Default: 30000
    Maximum delay cap in milliseconds
```

### Get a Setting

```bash
aperture config setting get default_timeout_secs
# Output: 30

aperture config setting get retry_defaults.max_attempts
# Output: 0
```

### Set a Setting

```bash
# Set request timeout to 60 seconds
aperture config setting set default_timeout_secs 60

# Enable JSON errors by default
aperture config setting set agent_defaults.json_errors true

# Enable automatic retries (3 attempts)
aperture config setting set retry_defaults.max_attempts 3

# Set initial retry delay to 1 second
aperture config setting set retry_defaults.initial_delay_ms 1000
```

Settings are validated against their expected types. Comments and formatting in `config.toml` are preserved.

## Global Configuration File

`~/.config/aperture/config.toml` stores global settings:

```toml
# Default timeout for HTTP requests (seconds)
default_timeout_secs = 30

[agent_defaults]
# Default settings for agent mode
json_errors = false

[retry_defaults]
# Automatic retry configuration
max_attempts = 3           # 0 = disabled
initial_delay_ms = 500     # Starting delay for exponential backoff
max_delay_ms = 30000       # Maximum delay cap (30 seconds)

[api_configs.my-api]
# Per-API base URL override
base_url_override = "https://api.example.com"

# Environment-specific URLs
[api_configs.my-api.environment_urls]
dev = "https://dev.api.example.com"
staging = "https://staging.api.example.com"
prod = "https://api.example.com"

# Strict validation mode
strict_mode = false

# Secret mappings
[api_configs.my-api.secrets.bearerAuth]
source = "env"
name = "API_TOKEN"
```

## Environment Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `APERTURE_BASE_URL` | Global base URL override | `https://api.example.com` |
| `APERTURE_ENV` | Environment selector | `staging`, `prod` |
| `RUST_LOG` | Log level | `debug`, `info`, `warn` |

## Parameter References

Aperture supports OpenAPI parameter references for DRY specifications:

```yaml
components:
  parameters:
    userId:
      name: userId
      in: path
      required: true
      schema:
        type: string
      description: User identifier

paths:
  /users/{userId}:
    get:
      parameters:
        - $ref: "#/components/parameters/userId"
    delete:
      parameters:
        - $ref: "#/components/parameters/userId"
```

References are resolved during spec validation and cached in binary format.

## OpenAPI 3.1 Support

OpenAPI 3.1 requires an optional feature flag:

```bash
# Build with 3.1 support
cargo install aperture-cli --features openapi31

# Or build from source
cargo build --release --features openapi31
```

Without the feature, 3.1 specs produce an error with instructions to enable it.

## Command Reference

### Spec Management (`config api`)

| Command | Description |
|---------|-------------|
| `config api add <name> <path>` | Add specification |
| `config api add --strict <name> <path>` | Add with strict validation |
| `config api list` | List registered specs |
| `config api remove <name>` | Remove specification |
| `config api reinit --all` | Rebuild all caches |
| `config api reinit <name>` | Rebuild specific cache |

### URL Management (`config url`)

| Command | Description |
|---------|-------------|
| `config url set <name> <url>` | Set base URL |
| `config url set <name> --env <env> <url>` | Set environment URL |
| `config url get <name>` | Show URL config |
| `config url list` | Show all URL configs |

### Secret Management (`config secret`)

| Command | Description |
|---------|-------------|
| `config secret set <name> <scheme> --env <var>` | Map secret |
| `config secret set <name> --interactive` | Interactive setup |
| `config secret list <name>` | List secret mappings |
| `config secret remove <name> <scheme>` | Remove one secret mapping |
| `config secret clear <name> [--force]` | Clear all secret mappings |

### Cache Management (`config cache`)

| Command | Description |
|---------|-------------|
| `config cache stats <name>` | Show cache stats |
| `config cache clear <name>` | Clear response cache |

### Command Mapping (`config mapping`)

| Command | Description |
|---------|-------------|
| `config mapping set <name> --group <original> <new>` | Rename a tag group |
| `config mapping set <name> --operation <id> --name <n>` | Rename an operation |
| `config mapping set <name> --operation <id> --op-group <g>` | Move operation to group |
| `config mapping set <name> --operation <id> --alias <a>` | Add an alias |
| `config mapping set <name> --operation <id> --remove-alias <a>` | Remove an alias |
| `config mapping set <name> --operation <id> --hidden` | Hide from help |
| `config mapping set <name> --operation <id> --visible` | Unhide |
| `config mapping list <name>` | List all mappings |
| `config mapping remove <name> --group <original>` | Remove group mapping |
| `config mapping remove <name> --operation <id>` | Remove operation mapping |

### Settings Management (`config setting`)

| Command | Description |
|---------|-------------|
| `config setting list` | List all settings with values |
| `config setting list --json` | List settings as JSON |
| `config setting get <key>` | Get a setting value |
| `config setting set <key> <value>` | Set a setting value |

### Compatibility Aliases

Legacy flat commands remain available during migration. Examples:

| Legacy | Nested replacement |
|--------|--------------------|
| `config set-url ...` | `config url set ...` |
| `config get-url ...` | `config url get ...` |
| `config list-urls` | `config url list` |
| `config set-secret ...` | `config secret set ...` |
| `config clear-cache ...` | `config cache clear ...` |
| `config settings` | `config setting list` |
