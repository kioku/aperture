# Aperture CLI

Aperture is a command-line interface (CLI) that dynamically generates commands from OpenAPI 3.x specifications. It's designed to provide a secure, reliable, and introspectable "tool-use" endpoint for autonomous AI agents and automated systems.

## Features

- **OpenAPI-Native:** Directly consumes standard OpenAPI 3.x documents as the single source of truth
- **Dynamic & Performant:** Generates commands at runtime from pre-validated, cached API specifications
- **Agent-First Design:** Optimized for programmatic use with structured I/O, JSON output modes, and actionable errors
- **Secure & Robust:** Enforces strict separation of configuration from secrets using environment variables
- **Spec Validation:** Validates OpenAPI specs during registration with clear error messages for unsupported features
- **Parameter References:** Full support for OpenAPI parameter references (`$ref`) for DRY specifications
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

Authentication is handled through custom `x-aperture-secret` extensions in OpenAPI specs that map security schemes to environment variables:

```yaml
components:
  securitySchemes:
    apiToken:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
```

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

#### JQ Support (Pure Rust Implementation)

The `jq` feature enables advanced JSON filtering using JQ syntax. This provides a pure Rust implementation without requiring external dependencies:

```bash
# Install with JQ support
cargo install aperture-cli --features jq

# Build from source with JQ support
cargo build --release --features jq
cargo install --path . --features jq
```

When enabled, you can use advanced JQ filters:
```bash
# Extract specific fields
aperture api my-api get-users --jq '.[].name'

# Complex filtering
aperture api my-api get-data --jq '.items | map(select(.active)) | .[0:5]'
```

**Note:** Without the `jq` feature, only basic field access is supported (e.g., `.name`, `.data.items`). For full JQ functionality including array operations, filters, and transformations, enable the feature during compilation.

## Getting Started

### Basic Usage

```bash
# Register an API specification
aperture config add my-api ./openapi.yml

# List available APIs
aperture config list

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

# Extract specific fields with JQ filtering
aperture api my-api users list --jq '.[] | {name: .name, email: .email}'

# Complex JQ transformations
aperture api my-api get-data --jq '.items | map(select(.active)) | .[0:5]'

# JQ filtering also works with --describe-json
aperture api my-api --describe-json --jq '.commands.users'
aperture api my-api --describe-json --jq '.commands | to_entries | .[].value[] | select(.deprecated)'
```

### Batch Operations & Automation

For high-volume automation, Aperture supports batch processing with concurrency controls:

```bash
# Execute multiple operations from a batch file
aperture --batch-file operations.json --batch-concurrency 10

# Rate limiting for batch operations
aperture --batch-file operations.json --batch-rate-limit 50

# Analyze batch results with JQ filtering (requires --json-errors)
# Note: The final JSON summary is printed after all operations complete
aperture --batch-file operations.json --json-errors --jq '.batch_execution_summary.operations[] | select(.success == false)'

# Get summary statistics only
aperture --batch-file operations.json --json-errors --jq '{total: .batch_execution_summary.total_operations, failed: .batch_execution_summary.failed_operations}'

# Find slow operations (> 1 second)
aperture --batch-file operations.json --json-errors --jq '.batch_execution_summary.operations[] | select(.duration_seconds > 1)'
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

This project is currently in active development. See [docs/plan.md](docs/plan.md) for detailed implementation progress and [docs/architecture.md](docs/architecture.md) for the complete software design specification.

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details.
