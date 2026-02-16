# Changelog

All notable changes to this project will be documented in this file.

## [0.1.8] - 2026-02-15

### ⚙️ Miscellaneous Tasks

- Remove duplicate crates.io publish step
- Move entire release process to GitHub Actions
- Apply rustfmt formatting
- Add conventional commit message hook
- Add cargo-deny configuration
- Add Dependabot configuration for cargo and github-actions
- Upgrade CI workflow
- Upgrade release workflow
- Add exclude list to Cargo.toml for crates.io hygiene
- Add fail-fast: false to integration-tests matrix
- Bump oas3 from 0.17.0 to 0.20.1
- Replace bincode with postcard for cache serialization
- Add nix build and flake check steps to CI workflow
- Add nix result symlink to .gitignore

### 🐛 Bug Fixes

- Prevent panic on Unicode body truncation in logging
- Standardize redaction markers to [REDACTED]
- **cache:** Scrub auth headers before storing CachedRequestInfo
- **cache:** Skip caching for authenticated requests by default
- **deps:** Upgrade bytes to 1.11.1 to resolve RUSTSEC-2026-0007
- Validate API context names to prevent path traversal (#74)
- Address review findings for atomic I/O and advisory locking
- Resolve clippy warnings in cache_fingerprint_tests for CI
- Resolve ast-grep and clippy linting violations
- Honor global output format when api command uses default
- Resolve PR review findings for batch cache path and legacy docs
- Restore Zlib license in deny.toml
- Move unix-gated dependencies to cross-platform section
- Overlay command mapping fields onto --describe-json manifest
- Use effective display names in exec shortcut resolution paths
- Align mapping collision detection with generator name resolution
- Align search effective_command_path with generator name resolution
- Prevent secret removal from erasing command mappings and environment URLs
- Correct minor issues in search and shortcut resolution
- Align build_full_command group fallback with generator and search
- Add conflicts_with between --group and --operation in set-mapping
- Add conflicts_with between --group and --operation in remove-mapping
- Reject empty string values in command mapping configuration
- Avoid creating empty operation mapping entries
- Validate spec existence for mapping removals
- Honor mapped command names in help and listings
- Resolve mapped command names in operation translation

### 📚 Documentation

- Add homebrew and cargo-binstall installation options
- Add comprehensive debugging and logging guide
- Update debugging guide with new logging features
- **adr:** Document cache policy for authenticated requests
- Add Nix installation instructions to README
- Add ADR 009 for custom command mapping approach

### 🚀 Features

- Initialize tracing-subscriber for debug logging
- Add -v flag for debug logging control
- Create logging module with redaction utilities
- Integrate logging into HTTP request/response execution
- Enhance logging security with expanded redaction
- Add APERTURE_LOG_FILE for file-based logging
- Add dynamic x-aperture-secret redaction in logs
- Warn on invalid APERTURE_LOG_FORMAT values
- **cache:** Add allow_authenticated config option
- **cache:** Add scrub_auth_headers utility function
- Spec cache fingerprint invalidation when source file changes (#75)
- Add flake.nix with package variants and devShell
- Add CommandMapping and OperationMapping models to ApiConfig
- Add display_group, display_name, aliases, hidden fields to CachedCommand
- Apply command mappings during spec transformation
- Update command tree generator to use display names, aliases, and hidden
- Update search and shortcuts to recognize display names and aliases
- Include mapping fields in --describe-json capability manifest
- Add CLI commands for managing command mappings
- Add --remove-alias flag to config set-mapping command

### 🚜 Refactor

- Decouple execution core from CLI layer (clap, stdout) (#77)
- Resolve PR review findings for executor/CLI decoupling

### 🧪 Testing

- Add integration tests for logging functionality
- **cache:** Add security tests for auth header scrubbing
- Add domain, translate, render, and executor decoupling coverage
- Add integration tests for command mapping feature
- Add unit tests for command mapping config manager methods
- Add search tests for display_name, alias, and display_group matching

## [0.1.7] - 2026-01-26

### ⚙️ Miscellaneous Tasks

- Add ast-grep configuration with custom Rust linting rules
- Suppress no-println warnings with ast-grep-ignore comments
- Exclude files with test modules from no-unwrap rule
- Update slab to 0.4.11 to fix RUSTSEC-2025-0047
- Suppress ast-grep no-println warnings in output module
- Update dependencies to latest versions
- Upgrade toml from 0.8 to 0.9
- Upgrade reqwest from 0.12 to 0.13
- Improve pre-commit hooks for better local testing

### 🎨 Styling

- Fix ast-grep linting issues

### 🐛 Bug Fixes

- Use sibling relationship for #[cfg(test)] module exclusion
- Resolve clippy warnings in test files and align pre-commit with CI
- Parse EDITOR env var to support commands with arguments
- **ci:** Override target-cpu=native to prevent SIGILL on cached builds
- **test:** Handle Windows .exe extension in binary name assertion
- Make quiet mode behavior consistent across all commands
- Show data in quiet mode for list-secrets and list-urls
- Switch from native-tls to rustls for cross-compilation support
- Use aws-lc-rs crypto backend instead of ring for Apple Silicon support
- Initialize rustls crypto provider for test compatibility
- **ci:** Add --force to cargo-nextest install for cache consistency
- **ci:** Add fallback check for cargo-nextest availability
- Filter skipped endpoints from --describe-json manifest
- **config:** Add debug assertions for type mismatches in set_setting
- Commit changelog before cargo release

### 📚 Documentation

- Restructure README and split into focused documentation (#61)
- Update binary size to 4.0MB (aws-lc-rs crypto backend)
- **config:** Add checklist for adding new settings
- Add documentation for v0.1.7 features

### 🚀 Features

- **agent:** Expose response schemas in describe manifest (#60)
- Add quiet mode for agent-friendly output suppression
- Hide global flags from dynamic command help
- Add multi-platform release workflow with cargo-binstall support
- Auto-update homebrew formula on release
- Add endpoint statistics showing X of Y endpoints available
- Add toml_edit for comment-preserving config edits
- Add SettingKey enum and validation for config settings
- Add Set, Get, Settings subcommands to config
- Implement ConfigManager methods for settings
- Implement command handlers for set/get/settings
- **config:** Add timeout range validation
- Add Retry-After header parsing support
- Add RetryInfo and RetryResult for retry tracking
- Add RetryDefaults configuration struct
- Add retry settings to settings system
- Add retry-related command line flags
- Thread retry configuration through executor
- Wire retry configuration from CLI to executor
- Add retry fields to BatchOperation
- Integrate retry logic into batch execution
- Enhance JSON error output with detailed retry info

### 🚜 Refactor

- Replace .unwrap() with .expect() in production code
- Eliminate all nested if statements (138 violations)
- Eliminate all max-nesting-depth violations (21 errors)
- Refine no-nested-if rule to reduce false positives
- Fix remaining no-nested-if violations after rule refinement
- Consolidate bonus score helpers into single generic function
- **config:** Extract value_from_config helper to reduce duplication
- Extract print_settings_list to reduce nesting depth
- Improve retry logic code quality
- Remove unused retry_status_codes config field

### 🧪 Testing

- Add batch operations quiet mode integration test
- Add assertion verifying command count matches available endpoints
- Add unit tests for ConfigManager settings methods
- Add integration tests for config set/get/settings commands
- Add test verifying TOML comments are preserved by set_setting
- Add comprehensive unit tests for is_retryable_status
- Add integration tests for retry behavior
- Update settings count expectation for retry config

## [0.1.6] - 2025-11-12

### ⚙️ Miscellaneous Tasks

- Clean up unused imports and fix warnings
- Enhance release script with changelog automation

### 🐛 Bug Fixes

- Resolve panic conditions and improve error handling
- Resolve panic conditions and improve error handling for production stability
- Rename help command to docs to avoid clap conflict
- **docs:** Replace .unwrap() with .ok() for consistency
- Resolve compilation errors with openapi31 and jq features
- Resolve variable shadowing to enable jq feature compilation
- Migrate to jaq v2.x to resolve compilation and functionality issues
- Convert tags with spaces to kebab-case for CLI compatibility
- Convert tags to kebab-case in documentation generator
- Resolve clap panic from parameter name conflicts and add boolean parameter support
- Enforce required boolean parameters with proper validation
- Make boolean path parameters consistently optional
- Ensure boolean parameters work correctly in positional args mode

### 📚 Documentation

- Update CLAUDE.md to reflect working jq feature with v2.x
- Update README to reflect working jq feature

### 🚀 Features

- Add search command to CLI interface
- Add enhanced help with examples
- Improve error messages with smart suggestions
- Implement command shortcuts and aliases
- Integrate comprehensive documentation features
- Add original_tags field for full tag consistency in JSON manifests
- Add boolean header parameter support

### 🚜 Refactor

- Break down long generate_command_help function
- Move inline imports to module/function level for better performance and readability
- Eliminate remaining high-priority inline imports in production code
- Fix final production code inline import in transformer.rs
- Complete inline import elimination - move all remaining imports to proper scope
- Complete standardization of import patterns

### 🧪 Testing

- Fix list-commands assertion to expect 'General' instead of 'default'
- Add integration tests for --show-examples execution path

## [0.1.5] - 2025-08-23

### Release Highlights

- **⚠️ Breaking**: CLI parameter flags now use kebab-case (e.g., `--user-id` instead of `--userId`)
- **Performance**: Binary size reduced by 67% (11MB → 3.6MB), test suite 75% faster (~30s → ~8s)
- **New Feature**: Optional OpenAPI 3.1 support via `--features openapi31`
- **Architecture**: Error system consolidated from 47+ types to 8 categories
- **Developer Experience**: Added cargo-nextest support and optimized CI pipeline

### Build

- Add cargo-nextest configuration for optimized testing

### ⚙️ Miscellaneous Tasks

- Optimize GitHub Actions workflow for faster testing

### ⚡ Performance

- Implement binary caching in integration_tests.rs
- Migrate remaining tests to cached binary
- Implement MockServer pooling infrastructure

### 🐛 Bug Fixes

- Redact authentication headers in dry-run output
- Convert parameter flags to kebab-case for CLI consistency
- Use unified parser for --describe-json to support OpenAPI 3.1
- Correct OpenAPI 3.1 parser string replacement
- Preserve security schemes during OpenAPI 3.1 to 3.0 conversion
- Reduce binary size from 11MB to 3.6MB
- Resolve clippy warnings after error consolidation
- Complete source code error consolidation migration
- Resolve final compilation errors in error consolidation
- Replace unwrap() with expect() and update error documentation
- Replace sleep-based delays with faster cache TTL
- Add missing Command import in cli_tests.rs
- Resolve test timing and JSON parsing issues
- Address PR review issues and consolidate documentation
- Remove broken pooling infrastructure from test optimizations
- Correct nextest thread configuration syntax

### 📚 Documentation

- Update table of contents for JQ Support section
- Add comprehensive code-level optimization analysis and plan
- Add comprehensive documentation to error module
- Add comprehensive ADR for test suite optimization

### 🚀 Features

- Add OpenAPI 3.1 support with oas3 fallback parser
- Make OpenAPI 3.1 support optional via openapi31 feature flag
- Complete error consolidation with helper methods and direct usage migration
- Complete error type consolidation for binary size reduction

### 🚜 Refactor

- Optimize string allocations in error module using Cow
- Centralize string literals into constants module
- Split large execute_request function into smaller helpers
- Add ErrorKind and ErrorContext types
- Migrate specification errors to new structure
- Migrate authentication errors to new structure
- Migrate validation errors to new structure
- Migrate request/response errors to new structure
- Complete error consolidation migration
- Complete source code error consolidation
- Replace error macros with builder methods
- Split HttpRequest into Network and HttpRequest error kinds

### 🧪 Testing

- Add comprehensive integration tests for kebab-case parameters
- Update existing tests for kebab-case parameter flags
- Add shared test utilities module
- Add test categorization attributes

## [0.1.4] - 2025-08-01

### 🎨 Styling

- Remove emojis from interactive configuration output
- Remove all emojis from source code

### 🐛 Bug Fixes

- Include auth-related warnings when building skip_endpoints list
- Add base64 encoding for basic auth credentials
- Prevent header injection in custom headers
- Include auth scheme names in skip messages
- Update test assertion for enhanced error message format
- Replace unwrap() with expect() in interactive config loop
- Improve kebab-case conversion for operation names
- Convert tag names to lowercase for CLI consistency
- Improve Unicode handling and simplify kebab-case conversion
- Ensure JSON manifest tag consistency and improve kebab-case conversion
- Handle server URL templates without panicking
- Resolve formatting and linting issues for server variable feature
- Improve backward compatibility error handling for template URLs
- Improve server variable error handling in URL resolution
- Replace panic with graceful error handling in URL resolver
- Preserve empty string defaults for server variables
- Add URL encoding for server variable values

### 📚 Documentation

- Update documentation for custom HTTP authentication support
- Update documentation for partial API support with unsupported auth
- Clarify basic auth environment variable format
- Update documentation for interactive secret configuration
- Improve thread safety documentation in interactive timeout handling
- Update version references to v0.1.4
- Add table of contents to README
- Update documentation and reorganize historical files

### 🚀 Features

- Add support for custom HTTP authentication schemes
- Update validator to handle unsupported auth schemes in non-strict mode
- Update display logic to show auth-related skip reasons
- Add debug logging for auth header construction
- Add secrets field to ApiConfig for dynamic authentication
- Add set-secret and list-secrets CLI commands
- Implement secret management methods in ConfigManager
- Update authentication resolution to prioritize config secrets
- Add interactive input utilities for user prompts
- Add interactive secret configuration to ConfigManager
- Integrate interactive mode into set-secret CLI command
- Add input length limits and sanitization to interactive prompts
- Add graceful exit mechanisms for interrupted sessions
- Add comprehensive interactive testing infrastructure
- Add timeout mechanisms for user input sessions
- Add functional tests for authentication priority system
- Add secret removal commands and fix test race conditions
- Enhance error handling and validation with retry mechanisms
- Add server variable data models
- Implement server variable extraction in SpecTransformer
- Add --server-var CLI flag support
- Add server variable resolver with comprehensive validation
- Integrate server variable resolution into BaseUrlResolver
- Integrate server variable resolution into request execution
- Add template variable name validation

### 🚜 Refactor

- Introduce AuthScheme enum and improve code organization
- Improve code organization
- Convert interactive.rs to module structure
- Decompose large methods in ConfigManager
- Flatten nested if statements in add_authentication_header
- Extract to_kebab_case into shared utils module
- Replace duplicated to_kebab_case with shared implementation
- Flatten nested conditionals in contains_template_variables
- Improve code readability based on PR review feedback

### 🧪 Testing

- Add comprehensive tests for custom HTTP authentication schemes
- Add header injection validation tests
- Add missing test coverage for edge cases
- Verify auth scheme consistency
- Add comprehensive integration tests for secret configuration
- Add comprehensive tests for interactive secret configuration
- Enhance to_kebab_case unit tests with comprehensive edge cases
- Add integration tests for tag lowercase conversion
- Add comprehensive tests for server URL template detection

## [0.1.3] - 2025-07-22

### ⚡ Performance

- Optimize content type validation to single iteration

### 🐛 Bug Fixes

- Prevent stack overflow from circular parameter references
- Resolve parameter references in describe-json output
- Address PR review comments for batch JQ filtering
- Implement proper output suppression for batch operations
- Address PR review comments for strict flag implementation
- Address all PR review comments for partial spec acceptance
- Address PR review comments for reinit and UX improvements
- Handle endpoints with mixed content types correctly
- Address critical PR review issues for content type validation
- Support all JSON content type variants (+json suffix)
- Preserve strict mode preference during reinit
- Standardize warning display across commands

### 📚 Documentation

- Restore CHANGELOG.md release history
- Update README with parameter reference support

### 🚀 Features

- Add support for parameter references in OpenAPI specifications
- Enable JQ filtering for --describe-json output
- Add JQ filtering support for batch operations with --json-errors
- Add ValidationResult types with backward compatibility
- Add --strict flag to config add command
- Add validation mode to SpecValidator
- Add --strict flag for partial spec acceptance with warnings
- Add warnings for endpoints with mixed content types

### 🚜 Refactor

- Extract parameter reference resolution to shared module
- Reduce MAX_REFERENCE_DEPTH from 50 to 10
- Improve content type handling and update docs
- Flatten deeply nested code for improved readability

### 🧪 Testing

- Add comprehensive tests for circular parameter references
- Add coverage for parameter names with special characters
- Add missing test coverage for edge cases

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

### ⚙️ Miscellaneous Tasks

- Prepare for v0.1.1 release

### 🎨 Styling

- Remove emojis from error messages

### 📚 Documentation

- Add comprehensive code review and future improvements documentation

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

## [0.1.0] - 2025-06-30

### ⚙️ Miscellaneous Tasks

- Init and add project docs
- Update dependencies to latest compatible versions

### 🎨 Styling

- Apply cargo fmt formatting

### 🐛 Bug Fixes

- Add APERTURE_CONFIG_DIR support in main.rs
- Implement mutex-based test isolation and resolve clippy warnings
- Correct base URL resolution priority hierarchy
- Resolve parallel test execution issues with environment variables

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
- Upgrade openapiv3 dependency from 1.0.0 to 2.2.0
- Implement x-aperture-secret extension parsing in SecurityScheme transformation
- Implement global security inheritance for OpenAPI operations
- Prepare repository for open source release
- Rename package to aperture-cli for crates.io uniqueness

### 🚜 Refactor

- Move engine tests to tests directory
- Pass base url as parameter to fix test isolation

### 🧪 Testing

- Add comprehensive integration tests with wiremock
- Add comprehensive integration tests for agent features
- Update tests for new CachedSpec base URL fields
- Add OpenAPI spec fixtures with x-aperture-secret extensions
- Add comprehensive x-aperture-secret extension parsing integration tests

<!-- generated by git-cliff -->
