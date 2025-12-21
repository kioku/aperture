# Configuration Reference

Aperture stores configuration in `~/.config/aperture/`. This document covers all configuration options and commands.

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
aperture config add my-api ./openapi.yaml

# From URL
aperture config add my-api https://api.example.com/openapi.yaml

# With strict validation (reject unsupported features)
aperture config add --strict my-api ./openapi.yaml
```

### List Specifications

```bash
aperture config list
```

### Remove a Specification

```bash
aperture config remove my-api
```

### Reinitialize Cache

Rebuild all cached specifications:

```bash
aperture config reinit
```

## Base URL Management

Override the base URL defined in OpenAPI specs.

### Set Base URL

```bash
# Permanent override
aperture config set-url my-api https://api.example.com

# Environment-specific
aperture config set-url my-api --env staging https://staging.api.example.com
aperture config set-url my-api --env prod https://api.example.com
```

### View URL Configuration

```bash
# Single API
aperture config get-url my-api

# All APIs
aperture config list-urls
```

### URL Resolution Priority

1. **CLI argument** (for testing)
2. **Environment-specific config** (when `APERTURE_ENV` is set)
3. **Per-API override** (set via `config set-url`)
4. **`APERTURE_BASE_URL`** environment variable
5. **OpenAPI spec** server URL
6. **Fallback** `https://api.example.com`

### Using Environments

```bash
# Configure environments
aperture config set-url my-api --env dev https://dev.api.example.com
aperture config set-url my-api --env staging https://staging.api.example.com
aperture config set-url my-api --env prod https://api.example.com

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
aperture config set-secret my-api bearerAuth --env API_TOKEN

# Interactive setup
aperture config set-secret my-api --interactive

# List configured secrets
aperture config list-secrets my-api
```

## Cache Management

### Response Cache

```bash
# View cache statistics
aperture config cache-stats my-api

# Clear response cache
aperture config clear-cache my-api
```

### Specification Cache

```bash
# Rebuild all spec caches
aperture config reinit

# Remove and re-add specific spec
aperture config remove my-api
aperture config add my-api ./openapi.yaml
```

## Global Configuration File

`~/.config/aperture/config.toml` stores global settings:

```toml
# Default timeout for HTTP requests (seconds)
default_timeout_secs = 30

[agent_defaults]
# Default settings for agent mode
json_errors = false

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

### Spec Management

| Command | Description |
|---------|-------------|
| `config add <name> <path>` | Add specification |
| `config add --strict <name> <path>` | Add with strict validation |
| `config list` | List registered specs |
| `config remove <name>` | Remove specification |
| `config reinit` | Rebuild all caches |

### URL Management

| Command | Description |
|---------|-------------|
| `config set-url <name> <url>` | Set base URL |
| `config set-url <name> --env <env> <url>` | Set environment URL |
| `config get-url <name>` | Show URL config |
| `config list-urls` | Show all URL configs |

### Secret Management

| Command | Description |
|---------|-------------|
| `config set-secret <name> <scheme> --env <var>` | Map secret |
| `config set-secret <name> --interactive` | Interactive setup |
| `config list-secrets <name>` | List secret mappings |

### Cache Management

| Command | Description |
|---------|-------------|
| `config cache-stats <name>` | Show cache stats |
| `config clear-cache <name>` | Clear response cache |
