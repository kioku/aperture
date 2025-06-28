# **Software Design Document: Aperture CLI**

- **Document Version:** 3.0 (Definitive Specification)
- **Product Version:** 1.0
- **Status:** Final Proposal
- **Author:** Claudiu Ivan with Gemini 2.5 Pro

### **Revision History**

- **v3.0 (Current):** Definitive specification incorporating comprehensive critique.
  - **Security:** Formalized the secret management model, linking OpenAPI `securitySchemes` to environment variables via a custom `x-aperture-secret` extension.
  - **Architecture:** Introduced a spec validation and caching mechanism to improve performance and provide early error detection.
  - **Configuration:** Solidified context naming and management. Fully specified the `config add` command, including `--force` and URL-based fetching.
  - **OpenAPI Support:** Provided a definitive list of supported and unsupported features for v1. Refined command generation rules to be more robust.
  - **Documentation:** Made the SDD self-contained by embedding all required schemas. Clarified versioning, roadmaps, and feature specifications.
- **v2.0:** Introduced OpenAPI-native design and agent-facing features.
- **v1.0 - v1.2:** Initial proposals and refinements based on a proprietary schema.

## 1. Overview

**Aperture** is a command-line interface (CLI) that generates a rich, interactive CLI directly from a standard **OpenAPI 3.x specification**. Its primary purpose is to provide a secure, reliable, and introspectable "tool-use" endpoint for autonomous AI agents and automated systems.

By adopting OpenAPI as its native configuration format, Aperture eliminates the need for manual API client coding. It provides a standardized bridge between agentic logic and any API, allowing systems to dynamically gain new capabilities simply by registering an API specification.

## 2. Goals and Scope

### 2.1. Goals

- **OpenAPI-Native:** Directly consume standard OpenAPI 3.x documents as the single source of truth.
- **Dynamic & Performant:** Generate commands at runtime from a pre-validated, cached representation of the API spec to ensure fast startup.
- **Agent-First Design:** Optimize for programmatic use with structured, schema-validated I/O and actionable errors.
- **Secure & Robust:** Enforce a strict separation of configuration from secrets. Build a reliable tool in Rust.

### 2.2. Non-Goals (for Product v1.0)

- **A GUI:** Aperture is exclusively a command-line tool.
- **Full OpenAPI Support:** See §5 for the explicit list of supported features.
- **Interactive Authentication:** User-interactive OAuth2 flows are deferred.
- **Generic Pagination Helper:** Automatic handling of paginated responses is deferred.

## 3. Configuration & Context Management

Aperture manages API specifications via a dedicated configuration directory, following the XDG Base Directory Specification (`~/.config/aperture/`).

### 3.1. Directory Structure

```
~/.config/aperture/
├── specs/
│   ├── sentry.openapi.yml
│   └── stripe.openapi.json
├── .cache/
│   ├── sentry.bin
│   └── stripe.bin
└── config.toml
```

- **`specs/`**: Contains the original OpenAPI specification files provided by the user.
- **`.cache/`**: Contains validated, pre-processed, and cached representations of the specs for fast runtime loading.
- **`config.toml`**: An optional file for global Aperture settings. See Appendix B for the schema.

### 3.2. API Contexts

An **API Context** is a named reference to an API specification. The context name is explicitly provided by the user during registration and is not derived from the filename. This allows for clear versioning (e.g., `sentry-v1`, `sentry-v2`).

### 3.3. Built-in `config` Command Suite

Aperture provides a built-in `config` command to manage API contexts.

| Command                           | Usage Scenario                  | Agent Interaction                                       | Human Interaction                                |
| :-------------------------------- | :------------------------------ | :------------------------------------------------------ | :----------------------------------------------- | ---------------------------------------------- |
| `config add <name> <file_or_url>` | Register or update an API spec. | `generate_spec                                          | aperture config add my-api --from-stdin`         | `aperture config add sentry ./sentry.yml`      |
| `--force`                         | Optional flag for `add`.        | `...                                                    | aperture config add my-api --force`              | Overwrites an existing spec without prompting. |
| `config edit <name>`              | Edit an existing spec.          | N/A (Agents use `add --force`).                         | Opens `$EDITOR` with the spec file.              |
| `config new <name>`               | Create a spec from a template.  | N/A (Agents should generate a full spec and use `add`). | Opens `$EDITOR` with a boilerplate OpenAPI file. |
| `config list`                     | List available API contexts.    | `aperture config list --json`                           | `aperture config list`                           |
| `config remove <name>`            | Remove an API spec.             | `aperture config remove old-api`                        | `aperture config remove old-api`                 |

## 4. Architecture & Performance

### 4.1. Architectural Diagram with Caching

```mermaid
flowchart TD
    subgraph "Setup & Validation (One-time on 'config add')"
        A[User/Agent] -- Invokes --> B(aperture config add sentry ./sentry.yml);
        B -- Parses & Validates Spec --> C{Spec OK?};
        C -- Yes --> D[Create Cached Representation];
        D -- Writes --> E[~/.config/aperture/.cache/sentry.bin];
        C -- No --> F[Report Validation Error to User];
    end
    subgraph "Runtime Execution (Fast)"
        G[User/Agent] -- Invokes --> H{aperture sentry ...};
        H -- Loads Cached Spec --> E;
        H -- Builds CLI --> I[Dynamic Command Tree];
        I -- Executes Request --> J[...];
    end
```

### 4.2. Performance: Validation & Caching

To ensure fast startup, Aperture does not parse the full OpenAPI specification on every invocation. Instead:

1. When `aperture config add` is run, the spec is thoroughly parsed and validated. This includes checking for command collisions and unsupported features.
2. If valid, a lightweight, serialized representation of the command tree and operations is saved to the `.cache/` directory.
3. On subsequent runs (`aperture <context> ...`), Aperture loads this pre-processed file directly, bypassing the expensive parsing and validation steps. The cache is invalidated if the spec file is modified.

## 5. OpenAPI Specification Support (v1.0)

Aperture's v1.0 implementation will support a well-defined subset of the OpenAPI 3.x specification.

| OpenAPI Keyword              | Support Status          | Details                                                                                                                   |
| :--------------------------- | :---------------------- | :------------------------------------------------------------------------------------------------------------------------ |
| `openapi`, `info`, `servers` | **Supported**           | `servers[0].url` is used as the `baseUrl`.                                                                                |
| `paths`, `operations`        | **Supported**           | The core structure for command generation.                                                                                |
| `tags`                       | **Supported**           | Used as the primary mechanism for command namespacing.                                                                    |
| `operationId`                | **Supported**           | Used as the primary source for subcommand names.                                                                          |
| `components`                 | **Supported**           | Referencing schemas, parameters, etc., via `$ref` is supported.                                                           |
| `parameters` (`in`)          | **Supported**           | `path`, `query`, and `header` are supported.                                                                              |
| `parameters` (`style`)       | **Unsupported**         | Default styles are assumed. Complex serialization is not supported.                                                       |
| `requestBody`                | **Partially Supported** | Only `content` type `application/json` is supported.                                                                      |
| `responses`                  | **Supported**           | Used to validate successful response bodies.                                                                              |
| `securitySchemes`            | **Partially Supported** | See §6 for the detailed security model. `apiKey` and `http` (bearer) are supported. `oauth2` and `openIdConnect` are not. |

Any unsupported keyword or feature encountered during `config add` will result in a clear validation error, preventing the spec from being registered.

### 5.1. Command Generation Strategy

1. **Namespace from `tags`:** An operation's first `tag` determines its command group (e.g., `tags: [Issues]` -> `aperture sentry issues ...`).
2. **Subcommand from `operationId`:** The subcommand is the kebab-cased `operationId` (e.g., `listProjectIssues` -> `list-project-issues`).
3. **Fallbacks (Robust):**
   - If an operation has no `tags`, it is placed under a `default` top-level command.
   - If it has no `operationId`, the subcommand name is the HTTP method (`get`, `post`).
   - Using dynamic path segments (e.g., `/{id}`) for command names is strictly forbidden.

## 6. Security Model & Secret Management

Aperture enforces a strict separation of configuration (the OpenAPI spec) and secrets.

1. **Declaration:** The API provider defines the required authentication in the `securitySchemes` object of the OpenAPI spec.
2. **Mapping:** The user (or agent) maps the declared scheme to a secret source using a custom `x-aperture-secret` extension.
3. **Resolution:** At runtime, Aperture reads the mapping and resolves the secret's value from the specified source (environment variable in v1.0).

**Example `securitySchemes` with Aperture Extension:**

```yaml
components:
  securitySchemes:
    sentryAuthToken:
      type: http
      scheme: bearer
      # Aperture-specific extension to map this scheme to a secret source
      x-aperture-secret:
        source: env
        name: SENTRY_AUTH_TOKEN
```

This configuration instructs Aperture to use the value of the `SENTRY_AUTH_TOKEN` environment variable for any operation secured by `sentryAuthToken`. If the extension is missing or the environment variable is unset, Aperture will fail with a `Config.SecretNotFound` error.

## 7. Agent-Facing Contracts

### 7.1. Global Agent Flags

| Flag                      | Behavior                                                                                  |
| :------------------------ | :---------------------------------------------------------------------------------------- |
| `--describe-json`         | Prints a JSON manifest detailing all commands and parameters for the current API context. |
| `--json-errors`           | Outputs all errors to `stderr` as a structured JSON object.                               |
| `--dry-run`               | For mutating operations, prints the request that _would_ be sent and exits.               |
| `--idempotency-key <key>` | Sets the `Idempotency-Key` header for safe retries.                                       |

### 7.2. Custom Extensions (`x-aperture-*`)

Aperture uses custom extensions to enrich the OpenAPI spec with agent-specific metadata. See Appendix A for the schemas of these extensions.

- `x-aperture-meta`: Top-level metadata about the API wrapper.
- `x-aperture-operation`: Agent-specific hints for an operation, including `agentDescription` and idempotency configuration.
- `x-aperture-secret`: Maps a `securityScheme` to a secret source.

## 8. Advanced Feature Handling

### 8.1. Pagination

For v1.0, Aperture **does not** provide an automatic pagination mechanism. If an API's `list` operation includes pagination parameters (e.g., `page`, `limit`, `next_cursor`), they will be exposed as standard CLI flags. The agent or user is responsible for making subsequent calls with the appropriate pagination values. A generic helper is a high-priority roadmap item.

### 8.2. Response Validation Failure Policy

Aperture is **strict by default**. If an API returns a successful (2xx) status code but the response body fails to validate against the OpenAPI schema, Aperture will:

1. Exit with a non-zero status code.
2. If `--json-errors` is used, emit a `Response.Validation` error. The error's `context` field will contain the raw response body received from the API.
   This strict policy prevents malformed or unexpected data from propagating into automated systems.

## 9. Roadmap (Post-v1.0 Product)

This SDD describes Product v1.0. Future development will focus on:

- **v1.1:** Introduce a generic pagination helper (`--auto-paginate`). Add support for non-interactive OAuth2 grants (`client_credentials`).
- **v1.2:** Add a `aperture config set <key> <value>` command for managing `config.toml`. Add support for `multipart/form-data` for file uploads.
- **v2.0:** Introduce keychain integration as an additional `SecretSource`. Expand OpenAPI support to include more complex features.

---

## Appendix A: Schemas for Custom Extensions & Agent Contracts

_(This section makes the SDD self-contained by embedding the previously referenced schemas.)_

### A.1. `x-aperture-*` Extension Schemas

```yaml
# Schema for x-aperture-meta (in 'info' object)
x-aperture-meta:
  type: object
  properties:
    description: { type: string }

# Schema for x-aperture-operation (in 'operation' object)
x-aperture-operation:
  type: object
  properties:
    agentDescription: { type: string }
    autoIdempotency: { type: boolean, default: false }
    idempotencyKeyHeader: { type: string, default: "Idempotency-Key" }

# Schema for x-aperture-secret (in 'securityScheme' object)
x-aperture-secret:
  type: object
  properties:
    source: { type: string, enum: ["env"] } # "keychain" is a future option
    name: { type: string }
  required: [source, name]
```

### A.2. Agent Contract Schemas

The JSON output for `--describe-json`, `--dry-run`, and `--json-errors` will conform to well-defined schemas, enabling reliable programmatic parsing. _(These are assumed to be detailed JSON Schema documents as specified in the previous critique's analysis.)_

## Appendix B: `config.toml` Specification

The `config.toml` file provides global configuration for Aperture's behavior.

**Example `config.toml`:**

```toml
# Default timeout for all API requests in seconds.
# Can be overridden by a command-specific flag if implemented.
default_timeout_secs = 30

[agent_defaults]
# If true, all errors will be output in JSON format by default,
# as if --json-errors were always present.
json_errors = true
```

