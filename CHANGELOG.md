# Changelog

All notable changes to this project will be documented in this file.

## [0.1.2] - 2025-07-12

### ⚙️ Miscellaneous Tasks

- Rename phase3_integration_tests.rs to command_syntax_integration_tests.rs
- Prepare release v0.1.2

### ⚡ Performance

- Optimize cache version checking with global metadata file
- Optimize timeout test execution with configurable timeouts

### 🐛 Bug Fixes

- Implement critical Phase 1 stability fixes
- Replace jq-rs with jaq for pure Rust implementation
- Resolve issues and improve JQ implementation
- Address critical Phase 3 PR review issues

### 📚 Documentation

- Add core enhancement roadmap for v0.1.x series
- Add comprehensive Phase 3 features documentation

### 🚀 Features

- Implement context-aware error messages for HTTP failures
- Implement command discovery with list-commands subcommand
- Implement configuration re-initialization and cache versioning
- Implement remote spec support with non-breaking API design
- Implement advanced output formatting with --format flag
- Implement jq filtering support for response processing
- Add batch processing module scaffold
- Integrate batch processing into CLI
- Implement response cache infrastructure
- Add cache management CLI commands and global cache flags
- Integrate response caching into executor
- Implement experimental flag-based parameter syntax
- Stabilize flag-based parameter syntax as default

### 🚜 Refactor

- Remove redundant HttpError variant

### 🧪 Testing

- Add ignored tests for remote spec support
- Add ignored tests for JQ filtering feature
- Add comprehensive Phase 3 integration tests

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
