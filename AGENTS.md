# Agent Development Guide

This file provides guidance for AI agents working with the Aperture CLI codebase.

## Development Workflow

1. Use conventional commits for clear change tracking
2. Run the full quality gate pipeline: `cargo test && cargo fmt && cargo clippy`
3. Follow a Test-Driven Development approach
4. Focus on the modular architecture with clear separation of concerns

## Agent-Friendly Features

Aperture is designed with an "agent-first" philosophy:

- **Structured Output**: `--describe-json` provides machine-readable API capability manifests with response schemas
- **Error Handling**: `--json-errors` outputs structured error information
- **Safe Testing**: `--dry-run` allows request inspection without execution
- **Idempotency**: `--idempotency-key` enables safe automated retries

### Response Schema Information

The `--describe-json` manifest includes `response_schema` for each command:
- `content_type`: Expected response content type (e.g., "application/json")
- `schema`: JSON Schema representation of the response body
- `example`: Example response if available from the OpenAPI spec

This enables agents to understand API response structure before execution, reducing hallucinations in data extraction.

## Key Architectural Concepts

- **Separation of Concerns**: Configuration (OpenAPI specs) and secrets are strictly separated
- **Caching Strategy**: OpenAPI specs are validated once and cached for performance
- **Agent-First Design**: All features optimized for programmatic use
- **Executor/CLI Decoupling**: The execution core (`invocation.rs`, `engine/executor.rs`) is independent of the CLI framework

For architecture details, see [docs/architecture.md](docs/architecture.md). For user-facing documentation, see the [docs/](docs/) directory.
