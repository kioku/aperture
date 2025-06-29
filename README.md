# Aperture CLI

Aperture is a command-line interface (CLI) that dynamically generates commands from OpenAPI 3.x specifications. It's designed to provide a secure, reliable, and introspectable "tool-use" endpoint for autonomous AI agents and automated systems.

## Features

- **OpenAPI-Native:** Directly consumes standard OpenAPI 3.x documents as the single source of truth
- **Dynamic & Performant:** Generates commands at runtime from pre-validated, cached API specifications
- **Agent-First Design:** Optimized for programmatic use with structured I/O, JSON output modes, and actionable errors
- **Secure & Robust:** Enforces strict separation of configuration from secrets using environment variables
- **Spec Validation:** Validates OpenAPI specs during registration with clear error messages for unsupported features

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

## Installation

### Using Cargo (Recommended)

```bash
cargo install aperture
```

### Build from Source

```bash
git clone https://github.com/kioku/aperture.git
cd aperture
cargo install --path .

## Getting Started

### Basic Usage

```bash
# Register an API specification
aperture config add my-api ./openapi.yml

# List available APIs
aperture config list

# Execute API commands (dynamically generated from spec)
aperture my-api users list
aperture my-api users create --name "John Doe" --email "john@example.com"
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
APERTURE_ENV=staging aperture my-api users list
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
aperture my-api --describe-json

# Output errors as structured JSON
aperture my-api --json-errors users list

# Preview request without execution
aperture my-api --dry-run users create --name "Test"

# Add idempotency key for safe retries
aperture my-api --idempotency-key "unique-key" users create --name "Test"
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
