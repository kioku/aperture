# Aperture

A CLI that generates commands from OpenAPI specifications.

Aperture parses OpenAPI 3.x specs once, caches them, and exposes every operation as a CLI command with structured JSON output. Built for automation: AI agents, scripts, CI/CD pipelines.

## Why Aperture?

**Fast**: Sub-10ms startup time. Binary-cached specs eliminate parsing overhead.

**Small**: 4.0MB static binary. No runtime dependencies.

**Correct**: Structured JSON output. Machine-readable errors. Predictable exit codes.

**Secure**: Credentials resolved from environment variables only—never stored in config.

**Agent-First**: Self-describing capability manifests. Batch operations. Idempotency support.

## Performance

| Metric | Value |
|--------|-------|
| Binary size | 4.0 MB |
| Startup time | < 10 ms |
| Memory (typical) | 3-5 MB |
| Spec loading | O(1) from binary cache |

## Quick Start

```bash
# Install
cargo install aperture-cli

# Register an API
aperture config add petstore https://petstore3.swagger.io/api/v3/openapi.json

# Configure authentication
aperture config set-secret petstore api_key --env PETSTORE_API_KEY

# Discover available operations
aperture api petstore --describe-json

# Execute a command
aperture api petstore pet get-pet-by-id --petId 1
```

## Agent Integration

Aperture provides features specifically for programmatic use:

```bash
# Get capability manifest (for agent discovery)
aperture api my-api --describe-json

# Structured errors (for programmatic handling)
aperture api my-api --json-errors users list

# Preview without executing
aperture api my-api --dry-run users create --name "Test"

# Batch operations with concurrency control
aperture api my-api --batch-file operations.json --batch-concurrency 10
```

See [Agent Integration Guide](docs/agent-integration.md) for patterns and examples.

## Installation

### Homebrew (macOS/Linux)

```bash
brew install kioku/tap/aperture
```

### cargo-binstall (Pre-built Binaries)

```bash
cargo binstall aperture-cli
```

### Nix

```bash
# Install directly
nix profile install github:kioku/aperture

# With JQ support
nix profile install github:kioku/aperture#aperture-jq

# With all features
nix profile install github:kioku/aperture#aperture-full
```

### From crates.io

```bash
cargo install aperture-cli
```

### From Source

```bash
git clone https://github.com/kioku/aperture.git
cd aperture
cargo install --path .
```

### Optional Features

```bash
# Full JQ filtering support
cargo install aperture-cli --features jq

# OpenAPI 3.1 support
cargo install aperture-cli --features openapi31

# Both
cargo install aperture-cli --features "jq openapi31"
```

## Documentation

| Document | Description |
|----------|-------------|
| [User Guide](docs/guide.md) | Day-to-day usage, commands, output formats |
| [Agent Integration](docs/agent-integration.md) | Capability manifests, batch ops, integration patterns |
| [Security Model](docs/security.md) | Authentication, secrets, cache security |
| [Configuration](docs/configuration.md) | URLs, environments, cache, command mapping |
| [Debugging](docs/debugging.md) | Request/response logging, troubleshooting |
| [Architecture](docs/architecture.md) | Technical design and internals |
| [Contributing](CONTRIBUTING.md) | Development setup, testing, guidelines |

## Project Status

**Experimental**: Core functionality is implemented and tested. API and features may evolve based on usage and feedback.

## License

MIT License. See [LICENSE](LICENSE) for details.
