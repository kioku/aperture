# Gemini Development Guide  

This file provides guidance for Google's Gemini AI when working with the Aperture CLI codebase.

## Primary Documentation

For comprehensive development guidance, please refer to [CLAUDE.md](CLAUDE.md), which contains:

- Project overview and architecture
- Development commands and workflows
- Testing strategies and tools  
- Configuration management details
- Code quality standards

## Gemini-Specific Considerations

Aperture was co-designed with Gemini 2.5 Pro and embodies principles that align well with Gemini's capabilities:

### Code Quality Focus
- **Strict Linting**: Uses `cargo clippy` with pedantic and nursery rules
- **Comprehensive Testing**: 122+ tests covering unit, integration, and end-to-end scenarios
- **Type Safety**: Leverages Rust's type system for reliability
- **Error Handling**: Structured error types with actionable messages

### Architectural Patterns
- **Trait Abstractions**: File system operations use traits for testability
- **Dependency Injection**: Mock implementations for isolated testing
- **Modular Design**: Clear module boundaries with minimal coupling
- **Documentation**: Extensive inline documentation and ADRs

### Development Best Practices
1. **Test-Driven Development**: Write failing tests before implementation
2. **Atomic Commits**: Use conventional commits for clear change tracking
3. **Quality Gates**: Run `cargo test && cargo fmt && cargo clippy` before commits
4. **Documentation**: Update relevant docs for user-facing changes

### OpenAPI Expertise
The project heavily uses OpenAPI 3.x specifications with custom extensions:
- `x-aperture-secret` for authentication mapping
- Spec validation and caching for performance
- Dynamic CLI generation from API definitions

For detailed technical specifications and implementation guidance, see [CLAUDE.md](CLAUDE.md) and the [Software Design Document](docs/architecture.md).