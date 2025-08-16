# Aperture CLI

Aperture is a command-line interface (CLI) that dynamically generates commands from OpenAPI 3.x specifications. It's designed to provide a secure, reliable, and introspectable "tool-use" endpoint for autonomous AI agents and automated systems.

## Table of Contents

- [Features](#features)
- [Architecture](#architecture)
  - [Configuration Structure](#configuration-structure)
  - [Security Model](#security-model)
    - [Supported Authentication Schemes](#supported-authentication-schemes)
    - [Unsupported Authentication](#unsupported-authentication)
    - [Partial API Support](#partial-api-support)
    - [Dynamic Secret Configuration](#dynamic-secret-configuration)
  - [Parameter References](#parameter-references)
- [Installation](#installation)
  - [Using Cargo (Recommended)](#using-cargo-recommended)
  - [Build from Source](#build-from-source)
  - [Optional Features](#optional-features)
    - [JQ Support](#jq-support)
- [Getting Started](#getting-started)
  - [Basic Usage](#basic-usage)
  - [Base URL Management](#base-url-management)
  - [Server URL Template Variables](#server-url-template-variables)
  - [Agent-Friendly Features](#agent-friendly-features)
  - [Advanced Output Formatting](#advanced-output-formatting)
  - [Batch Operations & Automation](#batch-operations--automation)
  - [Response Caching](#response-caching)
  - [Command Syntax](#command-syntax)
  - [Exit Codes](#exit-codes)
- [Development](#development)
  - [Prerequisites](#prerequisites)
  - [Development Commands](#development-commands)
  - [Testing](#testing)
- [Project Status](#project-status)
- [License](#license)

## Features

- **OpenAPI-Native:** Directly consumes standard OpenAPI 3.x documents as the single source of truth
- **Dynamic & Performant:** Generates commands at runtime from pre-validated, cached API specifications
- **Agent-First Design:** Optimized for programmatic use with structured I/O, JSON output modes, and actionable errors
- **Secure & Robust:** Enforces strict separation of configuration from secrets using environment variables
- **Spec Validation:** Validates OpenAPI specs during registration with clear error messages for unsupported features
- **Parameter References:** Full support for OpenAPI parameter references (`$ref`) for DRY specifications
- **Server Variables:** Support for OpenAPI server URL templates with validation and defaults
- **Batch Processing:** Execute multiple operations concurrently with rate limiting and error handling
- **Response Caching:** Intelligent caching with TTL support for improved performance
- **Advanced Output:** Multiple output formats (JSON, YAML, table) with JQ-based filtering
- **Flag-Based Syntax:** Consistent `--flag` syntax for all parameters (with legacy positional support)

## Architecture

Aperture follows a two-phase approach:

1. **Setup Phase** (`aperture config add`): Parses, validates, and caches OpenAPI specifications
2. **Runtime Phase** (`aperture <context> <command>`): Loads cached specs for fast command generation and execution

### Configuration Structure

```
~/.config/aperture/
├── specs/           # Original OpenAPI specification files
├── .cache/          # Pre-processed binary cache files
└── config.toml      # Global configuration (optional)
```

### Security Model

Authentication is handled through custom `x-aperture-secret` extensions in OpenAPI specs that map security schemes to environment variables.

#### Supported Authentication Schemes

1. **API Key** (header, query, or cookie)
```yaml
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: API_KEY
```

2. **HTTP Bearer Token**
```yaml
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
```

3. **HTTP Basic Authentication**
```yaml
components:
  securitySchemes:
    basicAuth:
      type: http
      scheme: basic
      x-aperture-secret:
        source: env
        name: BASIC_CREDENTIALS  # Format: username:password (will be base64 encoded automatically)
```

4. **Custom HTTP Schemes** (Token, DSN, ApiKey, proprietary schemes)
```yaml
components:
  securitySchemes:
    # Common alternative to Bearer
    tokenAuth:
      type: http
      scheme: Token
      x-aperture-secret:
        source: env
        name: API_TOKEN
    
    # Sentry-style DSN authentication
    dsnAuth:
      type: http
      scheme: DSN
      x-aperture-secret:
        source: env
        name: SENTRY_DSN
    
    # Any custom scheme name
    customAuth:
      type: http
      scheme: X-CompanyAuth-V2
      x-aperture-secret:
        source: env
        name: COMPANY_TOKEN
```

All custom HTTP schemes are treated as bearer-like tokens and formatted as: `Authorization: <scheme> <token>`

#### Unsupported Authentication

The following authentication types require complex flows and are not supported:
- OAuth2 (all flows)
- OpenID Connect
- HTTP Negotiate (Kerberos/NTLM)
- HTTP OAuth scheme

#### Partial API Support

Starting from v0.1.4, Aperture handles APIs with unsupported features gracefully:

- **Non-Strict Mode (Default)**: APIs containing unsupported authentication schemes or content types are accepted
  - Only endpoints that require unsupported features are skipped
  - Endpoints with multiple authentication options (where at least one is supported) remain available
  - Clear warnings show which endpoints are skipped and why
  
- **Strict Mode**: Use the `--strict` flag with `aperture config add` to reject specs with any unsupported features

This allows you to use most endpoints of an API even if some require unsupported authentication methods or content types:

```bash
# Default behavior - accepts spec, skips unsupported endpoints with warnings
aperture config add my-api ./openapi.yml

# Strict mode - rejects spec if any unsupported features found
aperture config add --strict my-api ./openapi.yml
```

#### Dynamic Secret Configuration

Starting from v0.1.4, Aperture supports dynamic authentication configuration without modifying OpenAPI specifications. This allows you to:

- **Use unmodified third-party OpenAPI specs** - No need to fork and add `x-aperture-secret` extensions
- **Easy credential management** - Configure authentication through simple CLI commands
- **Environment flexibility** - Use different credentials per environment without spec changes
- **Credential rotation** - Update environment variables without editing specifications

**Configure secrets with CLI commands:**

```bash
# Direct configuration
aperture config set-secret myapi bearerAuth --env API_TOKEN
aperture config set-secret myapi apiKey --env MY_API_KEY

# Interactive configuration (guided setup)
aperture config set-secret myapi --interactive

# List configured secrets
aperture config list-secrets myapi
```

**Priority system:**
1. **Config-based secrets** (set via CLI commands) take highest priority
2. **x-aperture-secret extensions** (in OpenAPI specs) used as fallback
3. Clear error messages when neither is available

This feature maintains complete backward compatibility - existing `x-aperture-secret` extensions continue to work exactly as before, but can now be overridden by config-based settings.

### Parameter References

Aperture fully supports OpenAPI parameter references, allowing you to define reusable parameters:

```yaml
components:
  parameters:
    userId:
      name: userId
      in: path
      required: true
      schema:
        type: string
paths:
  /users/{userId}:
    get:
      parameters:
        - $ref: '#/components/parameters/userId'
```

## Installation

### Using Cargo (Recommended)

```bash
cargo install aperture-cli
```

### Build from Source

```bash
git clone https://github.com/kioku/aperture.git
cd aperture
cargo install --path .
```

### Optional Features

Aperture supports optional features that can be enabled during compilation:

#### JQ Support

Aperture provides JSON filtering capabilities through the `--jq` flag:

**Basic Filtering (Default)**
Without any special features, Aperture supports basic field access:
```bash
# Simple field extraction
aperture api my-api get-user --id 123 --jq '.name'
aperture api my-api get-data --jq '.results.items'

# Nested field access
aperture api my-api get-user --id 123 --jq '.address.city'
```

### Optional Features

**OpenAPI 3.1 Support**
The `openapi31` feature enables parsing of OpenAPI 3.1 specifications:

```bash
# Build with OpenAPI 3.1 support
cargo build --release --features openapi31
```

Without this feature, only OpenAPI 3.0.x specifications are supported. When a 3.1 spec is detected without the feature enabled, a helpful error message will guide you to rebuild with the feature.

**Advanced Filtering (Experimental)**
The `jq` feature flag enables advanced JSON filtering using a pure Rust JQ implementation:

```bash
# Build with JQ support (currently has known issues - see issue #25)
cargo build --release --features jq
```

⚠️ **Known Issue:** The advanced JQ feature (`--features jq`) currently has a bug where filters return the entire JSON document instead of filtered results. See [issue #25](https://github.com/kioku/aperture/issues/25) for details. For production use, we recommend using the default build without the `jq` feature flag.

**Supported without `jq` feature:**
- Basic field access: `.field`, `.nested.field`
- Array index access: `.items[0]`

**Requires `jq` feature (currently broken):**
- Complex filters: `.[] | select()`, `map()`, array slicing
- Pipe operations and transformations
- Advanced JQ syntax

## Getting Started

### Basic Usage

```bash
# Register an API specification
aperture config add my-api ./openapi.yml

# Register with strict validation (rejects specs with any unsupported features)
aperture config add --strict my-api ./openapi.yml

# List available APIs
aperture config list

# Configure authentication secrets (v0.1.4)
aperture config set-secret my-api bearerAuth --env API_TOKEN
aperture config set-secret my-api --interactive  # Guided configuration
aperture config list-secrets my-api

# Execute API commands (dynamically generated from spec)
aperture api my-api users list
aperture api my-api users create --name "John Doe" --email "john@example.com"
aperture api my-api users get-user-by-id --id 123
```

### Base URL Management

Aperture provides flexible base URL configuration for different environments:

```bash
# Set a custom base URL for an API (overrides spec and environment variables)
aperture config set-url my-api https://api.example.com

# Configure environment-specific URLs
aperture config set-url my-api --env staging https://staging.example.com
aperture config set-url my-api --env prod https://prod.example.com

# View current URL configuration
aperture config get-url my-api

# List all configured URLs across APIs
aperture config list-urls

# Use environment-specific URL
APERTURE_ENV=staging aperture api my-api users list
```

**URL Resolution Priority:**
1. Explicit test parameter (for testing)
2. Per-API configuration (with environment support)
3. `APERTURE_BASE_URL` environment variable (global override)
4. OpenAPI spec server URL (default)
5. Fallback URL (`https://api.example.com`)

### Server URL Template Variables

Starting from v0.1.4, Aperture supports OpenAPI server URL templates with variables:

```bash
# For APIs with templated server URLs like https://{region}.api.example.com/{version}
# Provide template variables using --server-var
aperture api my-api users list --server-var region=us --server-var version=v2

# Variables with enum constraints are validated
aperture api my-api users list --server-var region=invalid  # Error if 'invalid' not in enum

# Variables with defaults can be overridden
aperture api my-api users list --server-var env=staging  # Overrides default 'production'
```

**OpenAPI Specification Example:**
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
        description: API version
```

**Features:**
- **Validation:** Enum values are validated, invalid values are rejected
- **Defaults:** Variables with defaults are optional, others are required
- **URL Encoding:** Variable values are automatically URL-encoded
- **Error Messages:** Clear guidance when variables are missing or invalid

### Agent-Friendly Features

```bash
# Get JSON description of all available commands
aperture api my-api --describe-json

# Output errors as structured JSON
aperture api my-api --json-errors users list

# Preview request without execution
aperture api my-api --dry-run users create --name "Test"

# Add idempotency key for safe retries
aperture api my-api --idempotency-key "unique-key" users create --name "Test"

# Get specific user with flag-based syntax
aperture api my-api --dry-run users get-user-by-id --id 123
```

### Advanced Output Formatting

Aperture supports multiple output formats and data filtering:

```bash
# Output as formatted table
aperture api my-api users list --format table

# Output as YAML
aperture api my-api users list --format yaml

# Extract specific fields with JQ filtering (basic - works by default)
aperture api my-api users get-user --id 123 --jq '.name'
aperture api my-api users get-user --id 123 --jq '.email'

# Nested field access (works by default)
aperture api my-api get-data --jq '.data.items[0].name'

# Complex JQ transformations (requires --features jq, currently broken)
# aperture api my-api get-data --jq '.items | map(select(.active)) | .[0:5]'

# JQ filtering with --describe-json (basic access works)
aperture api my-api --describe-json --jq '.api.info.title'
aperture api my-api --describe-json --jq '.commands.users'
```

### Batch Operations & Automation

For high-volume automation, Aperture supports batch processing with concurrency controls:

```bash
# Execute multiple operations from a batch file
aperture --batch-file operations.json --batch-concurrency 10

# Rate limiting for batch operations
aperture --batch-file operations.json --batch-rate-limit 50

# Analyze batch results with JQ filtering (requires --json-errors)
# Basic field access (works by default):
aperture api my-api --batch-file operations.json --json-errors --jq '.batch_execution_summary.total_operations'
aperture api my-api --batch-file operations.json --json-errors --jq '.batch_execution_summary.failed_operations'

# Complex filters (require --features jq, currently broken):
# aperture api my-api --batch-file operations.json --json-errors --jq '.batch_execution_summary.operations[] | select(.success == false)'
# aperture api my-api --batch-file operations.json --json-errors --jq '{total: .batch_execution_summary.total_operations, failed: .batch_execution_summary.failed_operations}'
```

**Example batch file (JSON):**
```json
{
  "operations": [
    {
      "id": "get-user-1",
      "args": ["users", "get-user-by-id", "--id", "123"]
    },
    {
      "id": "get-user-2", 
      "args": ["users", "get-user-by-id", "--id", "456"]
    }
  ]
}
```

### Response Caching

Improve performance with intelligent response caching:

```bash
# Enable caching with default TTL (300 seconds)
aperture api my-api --cache users list

# Custom cache TTL
aperture api my-api --cache --cache-ttl 600 users list

# Disable caching
aperture api my-api --no-cache users list

# Manage cache
aperture config cache-stats my-api
aperture config clear-cache my-api
```

### Command Syntax

Aperture now uses flag-based syntax by default for all parameters:

```bash
# Default flag-based syntax (recommended)
aperture api my-api users get-user-by-id --id 123

# Legacy positional syntax (backwards compatibility)
aperture api my-api --positional-args users get-user-by-id 123
```

### Exit Codes

Aperture follows standard CLI conventions for exit codes:

- **0**: Success - all operations completed successfully
- **1**: Failure - one or more operations failed, including:
  - API request failures (4xx, 5xx errors)
  - Network connection errors
  - Authentication failures
  - Batch operations with any failed requests

For batch operations, Aperture exits with code 1 if ANY operation fails, making it easy to detect failures in CI/CD pipelines:

```bash
# Check batch success/failure
aperture --batch-file ops.json --json-errors
if [ $? -eq 0 ]; then
    echo "All operations succeeded"
else
    echo "Some operations failed"
fi

# Continue despite failures
aperture --batch-file ops.json --json-errors || true
```

## Development

This project is built with Rust and follows Test-Driven Development practices.

### Prerequisites

- Rust (latest stable version)
- Cargo

### Development Commands

```bash
# Build the project
cargo build

# Run all tests
cargo test

# Run tests for specific module
cargo test config_manager

# Format code
cargo fmt

# Check formatting and linting
cargo fmt --check
cargo clippy -- -D warnings

# Run with debug output
RUST_LOG=debug cargo run -- config list
```

### Testing

The project uses comprehensive testing strategies:

- **Unit Tests**: Located in `tests/` directory
- **Integration Tests**: End-to-end CLI testing using `assert_cmd`
- **HTTP Mocking**: API interaction testing using `wiremock`

```bash
# Run integration tests
cargo test --test integration_tests

# Run with HTTP mocking
cargo test --test executor_tests
```

## Project Status

**Experimental**: This project is in an experimental phase. While core functionality is implemented and tested, the API and features may change as we iterate based on usage and feedback. See [docs/architecture.md](docs/architecture.md) for the complete software design specification.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
