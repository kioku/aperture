# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### 🚀 Features

#### Phase 3: Automation at Scale & Experimental Syntax

- **Batch Processing**: Execute multiple API operations from JSON/YAML batch files with concurrency control and rate limiting
- **Response Caching**: Intelligent caching with TTL support for improved performance on repeated requests
- **Experimental Flag-Based Parameter Syntax**: New command syntax using flags for all parameters (including path parameters)
- **Cache Management**: New `config clear-cache` and `config cache-stats` commands for managing response caches

#### Detailed Phase 3 Features

- Add `--batch-file` flag for bulk operations with JSON/YAML support
- Add `--batch-concurrency` and `--batch-rate-limit` flags for batch processing control
- Add `--cache`, `--no-cache`, and `--cache-ttl` flags for response caching
- Add `--experimental-flags` flag for flag-based parameter syntax
- Implement batch processing module with `tokio::sync::Semaphore` for concurrency control
- Implement rate limiting using `governor` crate for batch operations
- Add comprehensive response cache infrastructure with TTL and cleanup
- Integrate response caching into HTTP request executor
- Add cache management CLI commands (`clear-cache`, `cache-stats`)
- Implement experimental command generation with flag-based parameters

### 🧪 Testing

- Add comprehensive Phase 3 integration tests (26 new tests)
- Add batch processing integration tests with JSON/YAML parsing
- Add response cache integration tests with TTL and expiration
- Add experimental flags integration tests with backwards compatibility
- Add wiremock-based API mocking for realistic testing scenarios

### 📚 Documentation

- Update README.md with Phase 3 features and usage examples
- Add dedicated Phase 3 features documentation (`docs/phase3_features.md`)
- Update CLI help text and command descriptions
- Add batch file format examples and best practices

## [0.1.1] - 2025-07-04

### 🚀 Features

- Add specific error variants to replace generic Config errors
- Enrich cached models with OpenAPI metadata for better agent support
- Redesign ApiCapabilityManifest for stable agent contract
- Implement manifest generation from original OpenAPI specs
- Add description and bearer_format fields to CachedSecurityScheme
- Add comprehensive x-aperture-secret validation

### 🚜 Refactor

- Extract validation and transformation logic into spec module
- Use new spec module components in ConfigManager
- Update error handling to use specific error variants
- Extract HTTP method arrays to shared helper function

### 🎨 Styling

- Remove emojis from error messages

## [0.1.0] - 2025-06-30

### 🚀 Features

- Initialise project and configure development workflow
- Define core application error enum and tests
- Implement data model for config.toml
- Implement security and secret source models
- Define data models for cached spec representation
- Create file system abstraction for testability
- Implement 'config add' spec validation and caching
- Implement list_specs function
- Implement list_specs and remove_spec functions
- Implement list_specs, remove_spec, and edit_spec functions
- Build clap interface for config command suite and fix related issues
- Implement OpenAPI validation and caching in add_spec
- Implement cached spec loader
- Implement dynamic command generator
- Complete phase 4 dynamic command generation and execution engine
- Implement full dynamic command generation from cached specs
- Implement http request executor with full functionality
- Restore tag-based namespace organization in generator
- Enhance error handling and help text for better UX
- Add global flags for agent features
- Add JSON serialization for structured error output
- Implement --describe-json capability manifest
- Implement --dry-run and --idempotency-key flags
- Integrate agent flags in application flow
- Add base URL support to cached specs and API configs
- Implement base URL resolver with priority hierarchy
- Integrate BaseUrlResolver with executor and agent manifest
- Add CLI management commands for base URL configuration
- Release process with standard Rust workflow
- Add security scheme models to cached spec representation
- Implement security scheme extraction from OpenAPI specs
- Implement authentication header building with security schemes
- Add custom header support with --header CLI flag
- Complete agent capability manifest security extraction
- Add comprehensive security and header integration tests
- [**breaking**] Upgrade openapiv3 dependency from 1.0.0 to 2.2.0
- Implement x-aperture-secret extension parsing in SecurityScheme transformation
- Implement global security inheritance for OpenAPI operations
- Prepare repository for open source release
- Rename package to aperture-cli for crates.io uniqueness

### 🐛 Bug Fixes

- Add APERTURE_CONFIG_DIR support in main.rs
- Implement mutex-based test isolation and resolve clippy warnings
- Correct base URL resolution priority hierarchy
- Resolve parallel test execution issues with environment variables

### 🚜 Refactor

- Move engine tests to tests directory
- Pass base url as parameter to fix test isolation

### 📚 Documentation

- Update plan progress
- Update plan.md to reflect Phase 2 completion
- Add README.md and MIT LICENSE
- Update plan.md to reflect Phase 3.1 and 3.2 completion
- Update plan.md to reflect Phase 3.3 completion
- Update plan.md to reflect Phase 3.4 completion
- Add CLAUDE.md guidance file and enhance README
- Update plan.md to reflect Phase 1-3 completion
- Add adr for dynamic command generation string lifetime approach
- Add adrs for http executor design and test isolation
- Add comprehensive base URL management documentation
- Add ADR-005 for security authentication and custom headers
- Update ADR-005 to reflect complete x-aperture-secret implementation
- Update documentation to reflect production-ready status

### 🎨 Styling

- Apply cargo fmt formatting

### 🧪 Testing

- Add comprehensive integration tests with wiremock
- Add comprehensive integration tests for agent features
- Update tests for new CachedSpec base URL fields
- Add OpenAPI spec fixtures with x-aperture-secret extensions
- Add comprehensive x-aperture-secret extension parsing integration tests

### ⚙️ Miscellaneous Tasks

- Init and add project docs
- Update dependencies to latest compatible versions

<!-- generated by git-cliff -->
