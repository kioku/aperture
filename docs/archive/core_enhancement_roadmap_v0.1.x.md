--

## **Aperture CLI: v0.1.x Core Enhancement Roadmap**

- **Document Version:** 3.2
- **Status:** Active Development Plan for Next Release Cycle

### **1. Introduction**

This document outlines the consolidated strategic roadmap for the next major feature release of Aperture CLI. It reflects a clear set of priorities focused on delivering a significantly more powerful, ergonomic, and stable tool within the `v0.1.x` release series.

The features are organized into three sequential implementation phases. Each phase builds upon the last, ensuring a logical progression from foundational stability improvements to advanced automation and configuration capabilities.

### **2. The "Core Enhancement" Milestone (Target: v0.1.2+)**

This milestone encompasses all prioritized features for the next release cycle.

---

#### **Phase 1: Foundational DX & Stability**

_Focus: Address the most immediate quality-of-life gaps and implement critical maintenance tooling. This phase strengthens the core user loop._

##### **Feature 1.1: Context-Aware Error Messages**

- **Core Concept:** Evolve error reporting from generic messages to provide specific, actionable troubleshooting advice based on the context of the failure.
- **User Value:** Transforms a frustrating failure into a guided debugging session. When an auth-related 401 occurs, the user is told _exactly_ which environment variable to check for the specific API they are using, drastically reducing time-to-resolution.
- **Architectural & Implementation Plan:**
  1.  **Context Propagation:** Ensure error-producing functions (in `engine::executor`, `config::manager`) have access to contextual information (e.g., API name, security scheme).
  2.  **Error Enrichment:** Use `Result::map_err` to wrap low-level errors into richer, typed variants from `src/error.rs` (e.g., `Error::HttpError`, `Error::SecretNotSet`).
  3.  **Intelligent Formatting (`src/main.rs`):** The `print_error` function will become a sophisticated dispatcher. It will `match` on the specific error variant and use the data within it to format a hyper-specific, helpful message. For `HttpError` with a 401 status, it will load the relevant `CachedSpec`, find the active security schemes, and suggest checking the environment variables mapped in `x-aperture-secret`.

##### **Feature 1.2: Command Discovery (`list-commands`)**

- **Core Concept:** A new subcommand, `aperture list-commands <context>`, that prints a clean, tree-like summary of all available commands for a given API.
- **User Value:** Provides an instant, high-level overview of an API's capabilities, making it vastly easier for users to discover how to use a newly configured API.
- **Architectural & Implementation Plan:**
  1.  **CLI Definition (`src/cli.rs`):** Add a `ListCommands { context: String }` variant to the `Commands` enum.
  2.  **Implementation (`src/main.rs`):** The handler will leverage existing components perfectly. It will load the `CachedSpec`, group the `CachedCommand`s in-memory by their primary tag, and then iterate through the groups to print a clean summary of `tag -> command_name`. This requires no new architectural components.

##### **Feature 1.3: Configuration Re-initialization & Cache Versioning**

- **Core Concept:** A subcommand, `aperture config reinit [--all | <context>]`, that safely regenerates binary cache files, coupled with an automatic version-mismatch detection system.
- **User Value:** Provides a crucial maintenance tool for users to resolve cache corruption and ensures smooth transitions between Aperture versions that may have different cache formats.
- **Architectural & Implementation Plan:**
  1.  **CLI Definition (`src/cli.rs`):** Add `Reinit { context: Option<String> }` to `ConfigCommands`.
  2.  **Cache Versioning (`src/cache/models.rs`):** Add a `cache_format_version: u32` field to the `CachedSpec` struct. This constant will be incremented in the Aperture source code whenever a breaking change is made to the cache format.
  3.  **Automatic Upgrade Check (`src/engine/loader.rs`):** The `load_cached_spec` function will compare the file's `cache_format_version` with the application's `const CACHE_FORMAT_VERSION`. A mismatch will result in a new `Error::CacheVersionMismatch`, prompting the user to run `config reinit`.

---

#### **Phase 2: Advanced Interaction & Configuration**

_Focus: Enhance the tool's data processing capabilities and streamline the initial configuration workflow._

##### **Feature 2.1: Remote Spec Support**

- **Core Concept:** Upgrade the `aperture config add` command to accept a URL as an input, allowing for direct registration of publicly hosted OpenAPI specifications.
- **User Value:** Massively simplifies the onboarding process for any API with a published spec. Users no longer need to manually download files, saving steps and reducing friction.
- **Architectural & Implementation Plan:**
  1.  **CLI Definition (`src/cli.rs`):** The `file` argument in `ConfigCommands::Add` will be renamed to `file_or_url` to reflect its new dual purpose.
  2.  **Logic Update (`src/config/manager.rs`):** The `add_spec` function will be updated. It will first check if the `file_or_url` input string starts with `http://` or `https://`.
  3.  **Secure Fetching:** If it is a URL, the manager will use a `reqwest` client to fetch the spec content. This HTTP client **must** be configured with sensible defaults: a request timeout (e.g., 30 seconds) and a strict response size limit (e.g., 5-10MB) to prevent abuse or denial-of-service vectors.
  4.  **Pipeline Integration:** Once the spec content is fetched into a string, it is passed into the _exact same_ existing validation and transformation pipeline that local files use. This perfectly reuses the existing, robust architecture.

##### **Feature 2.2: Advanced Output Formatting**

- **Core Concept:** Introduce new output formatters beyond JSON, accessible via a `--format <type>` flag.
- **User Value:** Allows users to consume API data in the most convenient format for their task—human-readable tables for quick inspection, or YAML for configuration-oriented systems.
- **Architectural & Implementation Plan:**
  1.  **CLI (`src/cli.rs`):** Add a global `--format <FORMAT>` argument.
  2.  **Executor Pipeline (`src/engine/executor.rs`):** Refactor the response handling logic. After deserializing a response to `serde_json::Value`, pass it to a new formatting stage.
  3.  **Formatter Trait:** Define a `trait Formatter` with a `fn format(&self, data: &serde_json::Value) -> Result<String, Error>` method and create implementations for JSON, Table (`tabled` crate), and YAML (`serde_yaml` crate).

##### **Feature 2.3: Response Filtering & Transformation (jq-like)**

- **Core Concept:** Allow users to apply a `jq`-like filter expression to the JSON response body before it is formatted for output via a `--jq <EXPRESSION>` flag.
- **User Value:** An immensely powerful feature for scripting and data exploration. It allows users to extract specific fields and transform data structures directly on the command line.
- **Architectural & Implementation Plan:**
  1.  **Executor Pipeline (`src/engine/executor.rs`):** Insert a new filtering stage between JSON deserialization and formatting.
      - **Pipeline Flow:** `HttpResponse -> Deserialized JSON -> [JAQ Filter Stage] -> [Formatter Stage] -> stdout`.
  2.  **Tooling:** Use the `jaq` crate. The filter stage will compile the user's expression, run the parsed `serde_json::Value` through the interpreter, and pass the result to the formatter.

---

#### **Phase 3: Automation at Scale & Stabilization Prep**

_Focus: Implement features for high-volume automation and introduce experimental syntax to prepare for a future breaking-change release._

##### **Feature 3.1: Bulk Operations & Request Caching**

- **Core Concept:** Implement batch processing (`--batch-file`) with concurrency controls, and introduce response caching (`--cache <ttl>`).
- **User Value:** Unlocks true automation at scale and dramatically improves performance for repeated tasks.
- **Architectural & Implementation Plan:**
  1.  **Batch Controller (`src/batch.rs`):** A new module will manage the batch execution loop, using `tokio::sync::Semaphore` for concurrency and `governor` for rate-limiting.
  2.  **Response Caching (`src/engine/executor.rs`):** A caching middleware layer will be added to the executor. It will generate a unique hash key for each request and perform a read-through cache lookup in `~/.config/aperture/.cache/responses/` before making a network call.

##### **Feature 3.2: Experimental Flag-Based Parameter Syntax**

- **Core Concept:** Introduce a new, experimental `--flag-based-parameters` global flag that changes the CLI generation to use `--flag` syntax for all parameters, including path parameters.
- **User Value:** Allows early adopters and scripters to begin using a more robust and predictable command syntax, providing valuable feedback before it becomes the default.
- **Architectural & Implementation Plan:**
  1.  **CLI Definition (`src/cli.rs`):** Add a new global boolean flag: `--flag-based-parameters`.
  2.  **Generator Logic (`src/engine/generator.rs`):** The `generate_command_tree` function will accept this flag. Inside `create_arg_from_parameter`, an `if` condition will check the flag's value to decide whether to generate a positional argument or a long flag for path parameters.

---

### **4. Feature Backlog (Post-v0.1.x Cycle)**

The following features are valuable but are deliberately deferred to maintain focus. They will be considered for a future `v0.2.0` milestone.

- **Query Parameter Authentication:**

  - **Core Concept:** Automatically inject authentication tokens/keys as query parameters based on `x-aperture-secret` mappings in an OpenAPI spec.
  - **User Value:** Provides native, seamless support for a common (though often discouraged) API authentication pattern, removing the need for users to manually construct URLs or pass keys as command arguments.
  - **Architectural Implication:** This is a natural extension of the existing authentication system. It requires modifying the `SpecTransformer` to capture `in: query` security schemes and enhancing the `engine::executor`'s URL-building logic to append these key-value pairs before request execution.

- **Configuration Profiles:**

  - **Core Concept:** A command suite (`aperture config profile ...`) to create, manage, and activate named collections of settings (e.g., base URLs, auth tokens) for different environments.
  - **User Value:** Radically simplifies switching between `dev`, `staging`, and `production` environments by replacing the need to manage multiple environment variables with a single `--profile <name>` flag.
  - **Architectural Implication:** Extends the `GlobalConfig` model in `src/config/models.rs` to include a map of profiles. The application's configuration loading sequence will be modified to first resolve the active profile, then use its values to inform the subsequent resolution of base URLs and secrets.

- **API Discovery:**

  - **Core Concept:** An interactive command (`aperture config add --interactive`) that guides a user through finding and configuring an API spec when they only know its base URL.
  - **User Value:** Greatly lowers the barrier to entry by automating the often-tedious process of locating the correct `openapi.yaml` file for a service.
  - **Architectural Implication:** Introduces a new `src/discovery` module responsible for probing common spec paths (e.g., `/openapi.json`, `/docs/swagger.json`). Its output would feed an interactive flow built with a crate like `dialoguer`, which orchestrates existing `config::manager` functions.

- **Plugin System:**
  - **Core Concept:** A formal, secure mechanism for loading third-party, dynamically compiled code to extend Aperture's core functionality.
  - **User Value:** Unlocks limitless customization. The community could add support for complex auth flows (OAuth2), custom output formatters (CSV), or pre-request validation hooks.
  - **Architectural Implication:** This is a high-complexity, v2.0-level feature. It requires defining a stable plugin API (Rust traits), a secure loading mechanism (`libloading`), and instrumenting the core execution pipeline with "hook points" for plugins. Requires careful sandboxing to prevent security vulnerabilities.
