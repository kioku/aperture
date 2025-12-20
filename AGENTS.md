# Agent Development Guide

This file provides guidance for AI agents working with the Aperture CLI codebase.

## Primary Documentation

For comprehensive development guidance, please refer to [CLAUDE.md](CLAUDE.md), which contains:

- Project overview and architecture
- Development commands and workflows  
- Testing strategies and tools
- Configuration management details
- Code quality standards

## Agent-Specific Notes

Aperture is designed with an "agent-first" philosophy, making it particularly suitable for autonomous AI development:

### Agent-Friendly Features
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

### Development Workflow for Agents
1. Follow the Test-Driven Development approach outlined in CLAUDE.md
2. Use conventional commits for clear change tracking
3. Run the full quality gate pipeline: `cargo test && cargo fmt && cargo clippy`
4. Focus on the modular architecture with clear separation of concerns

### Key Architectural Concepts
- **Separation of Concerns**: Configuration (OpenAPI specs) and secrets are strictly separated
- **Caching Strategy**: OpenAPI specs are validated once and cached for performance
- **Agent-First Design**: All features optimized for programmatic use

For detailed implementation guidance, architecture decisions, and testing strategies, see [CLAUDE.md](CLAUDE.md).