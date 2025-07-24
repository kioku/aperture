## **Implementation Plan: Aperture CLI (v1.0)**

This document outlines the development process and concrete steps required to implement the Aperture CLI as specified in the Software Design Document v3.0.

### **0. Guiding Principles & Process**

All development will adhere to the following principles:

1. **Test-Driven Development (TDD):** For every new piece of logic, a failing test will be written first, followed by the implementation that makes it pass.
2. **Continuous Verification:** After every small change, the full suite of tests (`cargo test`), formatters (`cargo fmt`), and linters (`cargo clippy`) will be run. The compiler and LSP are our primary, constant feedback mechanisms.
3. **Atomic, Conventional Commits:** Each commit will represent a single, logical unit of work and follow the [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) specification. This creates a clean, navigable history.
4. **Modularity:** Code will be organized into logical modules with clear, testable interfaces. Dependencies between modules should be minimized.

---

### **Phase 1: Foundation & Workflow Setup**

**Goal:** Prepare a pristine, modern Rust repository with automated quality gates.
**Dependencies:** None. This is the starting point.
**Parallelization:** These tasks are sequential.

- `[x]` **Task 1.1: Initialize Project & Dependencies**

  - **Action:** Create a new binary Rust project: `cargo new --bin aperture`.
  - **Action:** Add all core dependencies to `Cargo.toml` as identified in the SDD: `clap`, `serde`, `serde_yaml`, `serde_json`, `oas3`, `reqwest`, `tokio`, `thiserror`, `anyhow`, `shellexpand`, `toml`. Add development dependencies: `wiremock`, `assert_cmd`.
  - **Verification:** `cargo build` completes successfully.
  - **Commit:** `chore: Initialise project and add dependencies`

- `[x]` **Task 1.2: Configure Toolchain & Formatter**

  - **Action:** Create a `rust-toolchain.toml` file to pin the Rust version for consistency.
  - **Action:** Create a `rustfmt.toml` file to enforce a consistent code style.
  - **Verification:** `cargo fmt` runs and formats the default `main.rs`.
  - **Commit:** `chore: Configure rust toolchain and formatter`

- `[x]` **Task 1.3: Configure Linter (Clippy)**

  - **Action:** Add a `[lints.clippy]` section to `Cargo.toml` to enforce strict linting rules (e.g., `pedantic`, `nursery`).
  - **Verification:** `cargo clippy -- -D warnings` runs without errors on the default project.
  - **Commit:** `chore: Configure strict clippy linting rules`

- `[x]` **Task 1.4: Implement Git Hooks**

  - **Action:** Integrate `husky` to manage git hooks.
  - **Action:** Create a `pre-commit` hook that runs `cargo fmt --check && cargo clippy --no-deps -- -D warnings && cargo test`.
  - **Verification:** Make a non-compliant change; `git commit` should fail. Fix the change; `git commit` should succeed.
  - **Commit:** `chore: Implement pre-commit hooks for quality gates`

- `[x]` **Task 1.5: Setup Continuous Integration (CI)**
  - **Action:** Create a GitHub Actions workflow file (`.github/workflows/ci.yml`).
  - **Action:** The CI job will run on every push and pull request, executing the same checks as the pre-commit hook on a matrix of platforms (Linux, macOS, Windows).
  - **Verification:** Push the initial commits to a new GitHub repository. The CI action should trigger and pass.
  - **Commit:** `ci: Add initial CI workflow for testing and linting`

---

### **Phase 2: Core Data Models & Error Handling**

**Goal:** Create the type-safe Rust structs that represent all concepts in the SDD.
**Dependencies:** Phase 1 complete.
**Parallelization:** All tasks in this phase are independent and can be worked on in parallel.

- `[x]` **Task 2.1: Define Core Error Type**

  - **Action:** Create `src/error.rs`. Define a top-level `Error` enum using `thiserror` that will aggregate all possible failures (I/O, parsing, network, etc.).
  - **Test:** Create a `tests/error_tests.rs` file. Write unit tests to ensure error variants format into the expected user-facing messages.
  - **Commit:** `feat(error): Define core application error enum`

- `[x]` **Task 2.2: Implement Global Configuration Models**

  - **Action:** Create `src/config/models.rs`. Implement `GlobalConfig` to represent `config.toml`.
  - **Test:** In a unit test, provide a sample TOML string and use `toml::from_str` to deserialize it. Assert that the resulting struct has the correct field values.
  - **Commit:** `feat(config): Implement data model for config.toml`

- `[x]` **Task 2.3: Implement Security & Secret Models**

  - **Action:** In `src/config/models.rs`, implement the `SecretSource` and other structs required for the `x-aperture-secret` extension.
  - **Test:** Write a unit test to deserialize a sample YAML snippet of a `securityScheme` containing the `x-aperture-secret` extension.
  - **Commit:** `feat(config): Implement security and secret source models`

- `[x]` **Task 2.4: Implement Internal Cached Spec Representation**
  - **Action:** In a new `src/cache/models.rs`, define the simplified structs that will be stored in the binary cache file. These will be derived from the `oas3` types but optimized for quick loading. They should be serializable with `serde`.
  - **Test:** Write a unit test to serialize and then deserialize a sample cached struct to ensure correctness.
  - **Commit:** `feat(cache): Define data models for cached spec representation`

---

### **Phase 3: Configuration Management Subsystem**

**Goal:** Implement the `aperture config` command suite for managing API specs.
**Dependencies:** Phase 2 complete.
**Parallelization:** The `add`, `list`, and `remove` commands can be developed in parallel after the file system abstraction is in place.

- `[x]` **Task 3.1: Create File System Abstraction**

  - **Action:** Create `src/fs.rs`. Define a `FileSystem` trait and a default implementation using `std::fs`. This allows for mocking in tests.
  - **Test:** Write unit tests for the default implementation.
  - **Commit:** `feat(fs): Create file system abstraction for testability`

- `[x]` **Task 3.2: Implement `config add` Logic**

  - **Action:** Create `src/config/manager.rs`. Implement the `add_spec` function. This function will:
    1. Fetch the spec from a file or URL.
    2. Parse it using the `oas3` crate.
    3. **Validate** it against Aperture's supported feature set (see SDD §5).
    4. Transform it into the internal cached representation (from Task 2.4).
    5. Serialize and write the cached representation to `~/.config/aperture/.cache/`.
  - **Test:** Write extensive integration tests for `add_spec`. Use mock files: a valid spec, a spec with unsupported features, a spec with command collisions. Assert that the cache is created successfully or that the correct, specific `Error` variant is returned.
  - **Commit:** `feat(config): Implement 'config add' spec validation and caching`

- `[x]` **Task 3.3: Implement `config list`, `remove`, `edit`**

  - **Action:** In `config/manager.rs`, implement the remaining management functions. These will primarily interact with the file system abstraction.
  - **Test:** Write unit tests for each function, using a mocked file system to verify that the correct files are read, deleted, or that the correct editor command is generated.
  - **Commit:** `feat(config): Implement 'list', 'remove', and 'edit' subcommands`

- `[x]` **Task 3.4: Build the `config` CLI**
  - **Action:** In `src/cli.rs`, use `clap` to build the `aperture config` command and its subcommands (`add`, `list`, etc.), wiring them to the `config::manager` functions.
  - **Test:** Use the `assert_cmd` crate to write end-to-end tests. For example, run `aperture config list` and assert the output.
  - **Commit:** `feat(cli): Build clap interface for config command suite`

---

### **Phase 4: Dynamic Command Generation & Execution Engine**

**Goal:** Implement the core logic that reads a cached spec, builds a dynamic CLI, and executes API requests.
**Dependencies:** Phase 3 complete.
**Parallelization:** The `Generator` and `Executor` can be developed in parallel.

- `[x]` **Task 4.1: Implement Cached Spec Loader**

  - **Action:** Create `src/engine/loader.rs`. Implement a function to load and deserialize a `.bin` file from the cache directory based on a context name.
  - **Test:** Write a unit test that saves a mock cache file and asserts that the loader can read it back correctly.
  - **Commit:** `feat(engine): Implement cached spec loader`
  - **Status:** ✅ **COMPLETED** - Full implementation with comprehensive error handling and unit tests

- `[x]` **Task 4.2: Implement Dynamic Command Generator**

  - **Action:** Create `src/engine/generator.rs`. Implement a function that takes the cached spec data and recursively builds a `clap::Command` tree according to the rules in SDD §5.1.
  - **Test:** Write a unit test with a sample cached spec. Generate the `clap::Command` and then inspect the generated structure to assert that subcommands and flags are created as expected.
  - **Commit:** `feat(engine): Implement dynamic CLI generator from cached spec`
  - **Status:** ✅ **COMPLETED** - Dynamic CLI generation with tag-based organization and parameter mapping

- `[x]` **Task 4.3: Implement the HTTP Request Executor**

  - **Action:** Create `src/engine/executor.rs`. This module will be responsible for:
    1. Mapping the `clap::ArgMatches` back to the specific API operation.
    2. Resolving secrets via the `x-aperture-secret` mapping.
    3. Constructing the final URL with path and query parameters.
    4. Building the `reqwest::Client` and `reqwest::Request`.
    5. Executing the request.
    6. Validating the response status code and body against the spec.
  - **Test:** This is critical. Use `wiremock-rs`. Create tests that:
    - Mock an API endpoint.
    - Run the executor with specific CLI arguments.
    - Assert that the mock server received a request with the correct method, path, query params, headers (including auth), and body.
    - Assert that the executor correctly handles both successful responses and API error responses (e.g., 404, 500).
  - **Commit:** `feat(engine): Implement HTTP request executor and response validator`
  - **Status:** ✅ **COMPLETED** - Full HTTP executor with authentication, custom headers, and comprehensive error handling

- `[x]` **Task 4.4: Integrate the Engine into `main.rs`**
  - **Action:** Modify `main.rs`. After parsing the context name, it should invoke the loader, generator, and executor in sequence.
  - **Test:** Write full end-to-end tests with `assert_cmd` and a running `wiremock` server.
  - **Commit:** `feat(app): Integrate execution engine into main application flow`
  - **Status:** ✅ **COMPLETED** - Full pipeline integration with end-to-end testing

---

### **Phase 5: Agent-Facing Features & Finalization**

**Goal:** Implement the special flags that make Aperture a powerful tool for agents.
**Dependencies:** Phase 4 complete.
**Parallelization:** These features are independent and can be implemented in parallel.

- `[x]` **Task 5.1: Implement `--describe-json`**

  - **Action:** Add the global flag. If present, load the cached spec and serialize it into the specified JSON format. Print to stdout and exit.
  - **Test:** Use `assert_cmd` to run `aperture <context> --describe-json`. Capture the stdout and validate it against a JSON schema or a snapshot file.
  - **Commit:** `feat(agent): Implement --describe-json capability manifest`
  - **Status:** ✅ **COMPLETED** - Full capability manifest generation with security information extraction

- `[x]` **Task 5.2: Implement `--json-errors`**

  - **Action:** Add the global flag. Modify the top-level error handling in `main.rs` to serialize the `Error` enum to JSON and print to stderr if the flag is active.
  - **Test:** Use `assert_cmd` to trigger various known errors (e.g., pointing to a non-existent spec) with the flag enabled. Assert that the stderr output is the expected JSON.
  - **Commit:** `feat(agent): Implement --json-errors for structured error reporting`
  - **Status:** ✅ **COMPLETED** - Structured JSON error output for programmatic error handling

- `[x]` **Task 5.3: Implement `--dry-run` and Idempotency**
  - **Action:** Modify the `engine::executor`. Before executing the request, check for the `--dry-run` flag. Also, check for the auto-idempotency configuration and the `--idempotency-key` flag to add the correct header.
  - **Test:** For `--dry-run`, use `assert_cmd` to check the JSON output. For idempotency, use `wiremock-rs` to assert the header is present on the request.
  - **Commit:** `feat(agent): Implement --dry-run and idempotency controls`
  - **Status:** ✅ **COMPLETED** - Request introspection and idempotency support for safe automation

---

### **Phase 6: Documentation & Release Preparation**

**Goal:** Prepare the project for public consumption.
**Dependencies:** All previous phases complete.

- `[x]` **Task 6.1: Write Comprehensive Documentation**

  - **Action:** Create a `README.md` that explains the project's purpose, installation, and basic usage.
  - **Action:** Create user documentation (e.g., in a `/docs` directory) that details all commands, the `config.toml` file, and the supported OpenAPI features.
  - **Commit:** `docs: Write initial user and project documentation`
  - **Status:** ✅ **COMPLETED** - Comprehensive README, architecture docs, ADRs, and SECURITY.md

- `[x]` **Task 6.2: Prepare for Release**
  - **Action:** Integrate `cargo-release` to automate version bumping, tagging, and publishing.
  - **Action:** Enhance the `ci.yml` workflow to build release binaries for multiple targets and attach them to a GitHub Release when a tag is pushed.
  - **Commit:** `chore(release): Configure cargo-release and binary release workflow`
  - **Status:** ✅ **COMPLETED** - Release automation with GitHub Actions and crates.io publishing

---

## **Post-v1.0 Features Implemented**

Beyond the original plan, the following major features have been successfully implemented:

### **Phase 7: Base URL Management System**
- `[x]` **Flexible Base URL Configuration**: Per-API URL overrides with environment-specific support
- `[x]` **Priority Hierarchy**: Explicit param → Config override → Env var → Spec default → Fallback
- `[x]` **CLI Management**: `set-url`, `get-url`, `list-urls` commands for base URL management
- `[x]` **Environment Support**: `APERTURE_ENV` variable for environment-specific URLs

### **Phase 8: Comprehensive Security Implementation**
- `[x]` **x-aperture-secret Extension**: Full OpenAPI extension parsing and integration
- `[x]` **Authentication Support**: Bearer tokens, API keys, Basic auth, and custom HTTP schemes via environment variables
- `[x]` **Custom Headers**: `--header` flag with environment variable expansion
- `[x]` **Security Discovery**: Agent capability manifest includes security requirements
- `[x]` **Global Security Inheritance**: Proper OpenAPI 3.0 spec-level security handling

### **Phase 9: Open Source Release Preparation**
- `[x]` **Package Management**: Renamed to `aperture-cli` for crates.io uniqueness
- `[x]` **Changelog Generation**: Automated changelog from conventional commits
- `[x]` **Security Documentation**: Comprehensive SECURITY.md policy
- `[x]` **Quality Assurance**: 122 passing tests, zero clippy warnings, formatted code

---

## **Project Status: Production Ready** 🚀

**All planned phases completed successfully.** Aperture CLI v0.1.0 is ready for open source release with:

- ✅ **Full Feature Set**: Dynamic CLI generation, authentication, base URL management, agent features
- ✅ **Production Quality**: Comprehensive testing, error handling, documentation
- ✅ **Agent-First Design**: JSON output modes, capability manifests, structured errors
- ✅ **Security Model**: Environment variable-based authentication with strict separation
- ✅ **Release Readiness**: Automated publishing, quality gates, comprehensive changelog

**Installation**: `cargo install aperture-cli`  
**Usage**: `aperture config add api spec.yaml && aperture api command`

