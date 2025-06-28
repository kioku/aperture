# Aperture CLI

Aperture is a command-line interface (CLI) that dynamically generates commands from OpenAPI 3.x specifications. It's designed to provide a secure, reliable, and introspectable "tool-use" endpoint for autonomous AI agents and automated systems.

## Features

- **OpenAPI-Native:** Directly consumes standard OpenAPI 3.x documents.
- **Dynamic & Performant:** Generates commands at runtime from cached API specifications.
- **Agent-First Design:** Optimized for programmatic use with structured I/O and actionable errors.
- **Secure & Robust:** Enforces separation of configuration from secrets.

## Getting Started

*(More detailed instructions will be provided here upon release.)*

## Development

This project is built with Rust. Ensure you have Rust and Cargo installed.

To run tests:

```bash
cargo test
```

To check formatting and linting:

```bash
cargo fmt --check
cargo clippy -- -D warnings
```
