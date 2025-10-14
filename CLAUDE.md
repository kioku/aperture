# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Aperture is a Rust CLI tool that dynamically generates commands from OpenAPI 3.x specifications. It serves as a bridge between autonomous AI agents and APIs by consuming OpenAPI specs and creating a rich command-line interface with built-in security, caching, and agent-friendly features.

## Core Architecture

### Module Structure
- **`src/config/`**: Configuration management system
  - `manager.rs`: Handles `aperture config` commands (add, list, remove)
  - `models.rs`: Data structures for global config and security models
- **`src/cache/`**: Spec caching and validation
  - `models.rs`: Optimized cached representations of OpenAPI specs
- **`src/cli.rs`**: Clap-based CLI interface definitions
- **`src/error.rs`**: Centralized error handling using `thiserror`
- **`src/fs.rs`**: File system abstraction for testability

### Key Design Patterns
- **Separation of Concerns**: Configuration (OpenAPI specs) and secrets are strictly separated
- **Caching Strategy**: OpenAPI specs are validated once during `config add` and cached as binary files for fast runtime loading
- **Test-Driven Development**: All functionality is developed with comprehensive unit and integration tests
- **Agent-First Design**: Special flags like `--describe-json`, `--json-errors`, and `--dry-run` for programmatic use

## Development Commands

### Building and Testing
```bash
# Build the project
cargo build

# Run all tests (unit + integration)
cargo test

# Run tests for a specific module
cargo test config_manager

# Run a single test
cargo test test_add_spec_validation
```

### Code Quality
```bash
# Format code
cargo fmt

# Check formatting without changing files
cargo fmt --check

# Run linter with strict rules
cargo clippy -- -D warnings

# Run clippy on specific package only (no deps)
cargo clippy --no-deps -- -D warnings
```

### Project-Specific Commands
```bash
# Test the CLI end-to-end (uses assert_cmd)
cargo test --test integration_tests

# Run with debug output
RUST_LOG=debug cargo run -- config list

# Test with wiremock for HTTP mocking
cargo test --test executor_tests

# Test base URL management functionality
cargo test --test base_url_integration_tests

# Test Phase 3 features
cargo test --test batch_processing_integration_tests
cargo test --test response_cache_integration_tests
cargo test --test phase3_integration_tests
cargo test --test experimental_flags_tests
```

## Configuration Management

The project uses a structured configuration system:
- **Specs Directory**: `~/.config/aperture/specs/` - Original OpenAPI files
- **Cache Directory**: `~/.config/aperture/.cache/` - Binary cached representations
- **Response Cache**: `~/.config/aperture/.cache/response_cache/` - HTTP response caches
- **Global Config**: `~/.config/aperture/config.toml` - Application settings

### Security Model
Uses custom `x-aperture-secret` extensions in OpenAPI specs to map authentication schemes to environment variables, maintaining strict separation between configuration and secrets.

### Command Syntax (v0.1.2+)
- **Default**: Flag-based syntax for all parameters (`--id 123`)
- **Legacy**: Positional syntax available with `--positional-args` flag
- **Examples**: `aperture api my-api users get-user-by-id --id 123`

## Testing Strategy

### Test Organization
- **Unit Tests**: In `tests/` directory with `_tests.rs` suffix (~0.2s runtime)
- **Integration Tests**: Full end-to-end testing using `assert_cmd` and `wiremock` (~9s runtime)
- **Mock Dependencies**: File system operations use trait abstractions for testability
- **Performance**: Optimized test suite runs in ~10s total (87% improvement from original ~2min)

### Key Testing Tools
- `assert_cmd`: CLI testing framework
- `wiremock`: HTTP mocking for API interactions
- `predicates`: Assertion helpers for complex conditions
- `cargo-nextest`: Enhanced test runner with better parallelization (optional but recommended)

### Test Execution
- **Unit tests only**: `cargo test --no-default-features`
- **Full suite**: `cargo test --features integration`
- **With nextest**: `cargo nextest run --profile fast` or use `./scripts/test-fast.sh`

## Implementation Phases

The project follows a structured development approach:
1. **Foundation**: Project setup, dependencies, quality gates
2. **Core Models**: Error handling, configuration, caching data structures
3. **Config Management**: `aperture config` command suite
4. **Dynamic Generation**: Runtime CLI building from cached specs
5. **Agent Features**: Special flags and JSON output modes
6. **Documentation**: User guides and release preparation

## Dependencies

### Core Runtime
- `clap`: CLI argument parsing with derive macros
- `openapiv3`: OpenAPI 3.x specification parsing
- `reqwest`: HTTP client for API requests
- `serde`: Serialization ecosystem
- `tokio`: Async runtime

### Optional Dependencies (Feature-Gated)
- `jaq-core`, `jaq-json`, `jaq-std`: Pure Rust JQ implementation v2.x (enabled with `--features jq`)
- `ahash`: High-performance hashing for JQ support

### Development/Testing
- `assert_cmd`: Command-line testing
- `wiremock`: HTTP mocking
- `predicates`: Test assertions

## Feature Flags

### `jq` Feature
Enables advanced JSON filtering using JQ syntax with a pure Rust implementation (jaq v2.x):
- **Build:** `cargo build --features jq`
- **Test:** `cargo test --features jq`
- **Without feature:** Only basic field access works (`.field`, `.nested.field`)
- **With feature:** Full JQ syntax support including nested fields, array operations, and filters

**Implementation:** Uses jaq-core, jaq-json, and jaq-std for a pure Rust implementation of JQ. This feature enables filtering of API responses, batch operation outputs, and describe-json results using standard JQ syntax.

### `openapi31` Feature
Enables support for OpenAPI 3.1 specifications:
- **Build:** `cargo build --features openapi31`
- **Test:** `cargo test --features openapi31`
- **Without feature:** Only OpenAPI 3.0.x specs are supported. Attempting to add a 3.1 spec will result in an error with instructions to enable the feature.
- **With feature:** Both OpenAPI 3.0.x and 3.1.x specs are supported. The oas3 crate is used to parse 3.1 specs and convert them to the 3.0 format internally.

**Note:** This feature adds the oas3 dependency which increases binary size. Only enable if you need OpenAPI 3.1 support.

## Code Style

The project enforces strict code quality through:
- **Rustfmt**: Consistent formatting (see `rustfmt.toml`)
- **Clippy**: Pedantic and nursery lints enabled
- **Pre-commit hooks**: Automated quality checks
- **CI/CD**: GitHub Actions for cross-platform testing