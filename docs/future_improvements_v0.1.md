# Aperture CLI - Future Improvements

Based on comprehensive testing across multiple APIs (Assembla, PokéAPI, OpenWeatherMap, Sentry), this document outlines areas for enhancement to improve user experience and functionality. Each improvement is analyzed for architectural impact and integration with Aperture's existing module structure.

## 🏗️ Architectural Context

Aperture follows key design patterns that must be preserved during improvements:

- **Separation of Concerns**: Configuration (OpenAPI specs) and secrets are strictly separated
- **Caching Strategy**: OpenAPI specs are validated once during `config add` and cached as binary files
- **Agent-First Design**: Special flags like `--describe-json`, `--json-errors`, and `--dry-run` for programmatic use
- **Test-Driven Development**: All functionality developed with comprehensive unit and integration tests

### Module Structure Overview
- **`src/config/`**: Configuration management system (`manager.rs`, `models.rs`, `url_resolver.rs`)
- **`src/cache/`**: Spec caching and validation (`models.rs`)
- **`src/engine/`**: Dynamic CLI generation (`generator.rs`, `executor.rs`, `loader.rs`)
- **`src/cli.rs`**: Clap-based CLI interface definitions
- **`src/error.rs`**: Centralized error handling using `thiserror`
- **`src/agent.rs`**: Agent-friendly feature implementations

## 🎯 High Priority Improvements

### 1. Query Parameter Authentication Support
**🏗️ Architectural Impact: HIGH** | **Module: `src/config/models.rs`, `src/engine/executor.rs`**

**Current Limitation:** Environment variable injection only works for header-based authentication
```bash
# Currently requires manual parameter
aperture api openweathermap default get-forecast --lat "44.4268" --appid "$OPENWEATHERMAP_API_KEY"
```

**Proposed Solution:** Automatic query parameter injection
```bash
# Should work automatically
aperture api openweathermap default get-forecast --lat "44.4268"
# With OPENWEATHERMAP_API_KEY automatically injected as ?appid=...
```

**Architectural Integration:**
- **Extends** existing `x-aperture-secret` processing in `SecurityScheme` handling
- **Modifies** request building pipeline in `src/engine/executor.rs`
- **Preserves** config/secrets separation principle
- **Maintains** same environment variable mapping patterns from header auth

**Implementation Strategy:**
- Extend `SecurityScheme` enum in `src/cache/models.rs` to handle query parameter injection
- Add query parameter injection logic to request builder in executor
- Update dry-run output to show injected query parameters
- Ensure cached specs regenerate to include new security handling

### 2. Parameter Syntax Standardization
**🏗️ Architectural Impact: MEDIUM** | **Module: `src/engine/generator.rs`**

**Current State:** Inconsistent parameter styles across different APIs
```bash
# Positional parameters
aperture api assembla tickets get-ticket-by-number "space" "ticket"

# Flag-based parameters  
aperture api openweathermap default get-forecast --lat "44.4268"

# Mixed approaches
aperture api sentry default get-project-issues "org" "project" --statsPeriod "14d"
```

**Proposed Solution:** Standardize to consistent flag-based syntax
```bash
# Standardized approach
aperture api assembla tickets get-ticket-by-number --space-id "tech-demonstrator" --ticket-number "3501"
aperture api openweathermap default get-forecast --lat "44.4268" --lon "26.1025"
aperture api sentry default get-project-issues --org "ouro" --project "core" --stats-period "14d"
```

**Architectural Integration:**
- **Modifies** command generation logic in `src/engine/generator.rs`
- **Extends** existing `to_kebab_case()` function for parameter naming
- **Preserves** binary cache compatibility through versioning
- **Maintains** existing clap Command structure

**Implementation Notes:**
- Add feature flag for gradual rollout: `--use-flag-parameters`
- Maintain backward compatibility with current positional parameters
- Add aliases for common parameter names (`--org` / `--organization`)
- Generate parameter names from OpenAPI parameter definitions
- Update cached command generation to include both parameter styles

### 3. Enhanced Error Messages
**🏗️ Architectural Impact: LOW** | **Module: `src/error.rs`**

**Current State:** Generic error messages with limited actionable guidance
```
🚫 Configuration Error
Request failed with status 401 Unauthorized
```

**Proposed Improvements:**
```
🚫 Authentication Error (401 Unauthorized)
API key invalid or expired for OpenWeatherMap API.

Troubleshooting:
• Check environment variable: OPENWEATHERMAP_API_KEY
• Verify API key at: https://openweathermap.org/api_keys
• Test connection: aperture api openweathermap --dry-run --describe-json

Need help? Run: aperture api openweathermap --help
```

**Architectural Integration:**
- **Extends** existing error types in `src/error.rs` using `thiserror`
- **Leverages** context from cached specs for API-specific guidance
- **Integrates** with agent-friendly error output (`--json-errors`)
- **Maintains** structured error propagation through Result types

**Error Categories to Enhance:**
- Authentication failures (401/403) - Reference environment variables from security schemes
- Missing parameters (400) - Show required parameters from OpenAPI spec
- Rate limiting (429) - Suggest retry strategies with exponential backoff
- API unavailability (5xx) - Provide status page links if available in spec
- Configuration issues - Guide to `aperture config` commands
- Network connectivity problems - Suggest `--dry-run` for debugging

## 🚀 Medium Priority Enhancements

### 4. Configuration Management UX
**🏗️ Architectural Impact: MEDIUM** | **Module: `src/config/manager.rs`, `src/config/url_resolver.rs`**

**Current Limitations:**
- Only supports local file paths
- No validation feedback during configuration
- Limited discovery capabilities

**Architectural Integration:**
- **Extends** `src/config/url_resolver.rs` for remote spec fetching
- **Enhances** `src/config/manager.rs` with validation feedback
- **Leverages** existing binary caching in `src/cache/models.rs`
- **Preserves** configuration storage patterns in `~/.config/aperture/`

**Proposed Features:**

#### Remote Spec Support
```bash
# Support direct URLs
aperture config add github-api https://api.github.com/openapi.json

# Support popular API registries
aperture config add stripe --registry=apis.guru
```

**Implementation Strategy:**
- Extend `UrlResolver` to handle HTTPS downloads with validation
- Add registry support through well-known spec URLs
- Cache remote specs locally following existing binary cache pattern

#### Auto-Discovery
```bash
# Discover OpenAPI specs
aperture config discover https://api.example.com/
# → Found: /openapi.json, /docs/swagger.json

# Interactive configuration
aperture config add myapi --interactive
# → Prompts for URL, auth method, environment variables
```

**Implementation Strategy:**
- New `src/discovery/` module with common OpenAPI spec path patterns
- Interactive mode using `dialoguer` crate for user prompts
- Integration with existing configuration validation pipeline

#### Validation & Feedback
```bash
# Enhanced feedback during add
aperture config add myapi spec.yaml
✅ Spec validated successfully
📊 Discovered 15 operations across 3 categories:
   • users (5 operations)
   • orders (7 operations) 
   • products (3 operations)
🔐 Authentication: Bearer token (MYAPI_TOKEN required)
```

**Implementation Strategy:**
- Enhance validation output in `config::manager::add_spec()`
- Parse and summarize operations during spec processing
- Display security requirements from `x-aperture-secret` extensions

### 5. Output Formatting Options
**🏗️ Architectural Impact: MEDIUM** | **Module: `src/engine/executor.rs`, `src/agent.rs`**

**Current State:** JSON-only output

**Architectural Integration:**
- **Extends** response processing pipeline in `src/engine/executor.rs`
- **Leverages** existing agent-friendly patterns in `src/agent.rs`
- **Preserves** structured output for `--describe-json` and `--json-errors`
- **Maintains** async execution patterns with tokio

**Proposed Formats:**

#### Table Format
```bash
aperture api pokeapi pokemon api_v2_pokemon_list --format table --limit 5
┌─────────────┬─────┬─────────────────────────────────────┐
│ Name        │ ID  │ URL                                 │
├─────────────┼─────┼─────────────────────────────────────┤
│ bulbasaur   │ 1   │ https://pokeapi.co/api/v2/pokemon/1 │
│ ivysaur     │ 2   │ https://pokeapi.co/api/v2/pokemon/2 │
│ venusaur    │ 3   │ https://pokeapi.co/api/v2/pokemon/3 │
└─────────────┴─────┴─────────────────────────────────────┘
```

#### YAML Output
```bash
aperture api sentry issues list --format yaml --limit 1
```

#### Minimal Output
```bash
aperture api pokeapi pokemon api_v2_pokemon_retrieve pikachu --quiet
# Only essential data, no execution info
```

#### Verbose Mode
```bash
aperture api assembla tickets get-ticket-by-number "tech-demonstrator" "3501" --verbose
🔗 Request: GET https://api.assembla.com/v1/spaces/tech-demonstrator/tickets/3501
📤 Headers: x-api-key, x-api-secret, user-agent
⏱️  Response time: 234ms
📥 Response size: 2.1KB
✅ Status: 200 OK
```

**Implementation Strategy:**
- Add `OutputFormat` enum to `src/engine/executor.rs` with variants: Json, Table, Yaml, Minimal
- Integrate with existing agent response handling patterns
- Use `tabled` crate for table formatting, preserving async patterns
- Extend `--describe-json` to include format options metadata
- Ensure verbose mode integrates with existing request timing infrastructure

## 🌟 Future Feature Requests

### 6. Response Filtering & Transformation
**🏗️ Architectural Impact: HIGH** | **Module: `src/engine/executor.rs`, `src/filters/` (new)**

**JQ Integration:**
```bash
# Filter with jq syntax
aperture api sentry issues list --jq '.[] | select(.level == "error")'

# Field selection
aperture api pokeapi pokemon api_v2_pokemon_retrieve pikachu --fields "name,abilities,types"
```

**Path-based Filtering:**
```bash
# Extract specific fields
aperture api openweathermap forecast --extract "list[].main.temp_max"
```

**Architectural Integration:**
- **Creates** new `src/filters/` module for transformation pipeline
- **Extends** response processing in `src/engine/executor.rs`
- **Integrates** with existing JSON response handling
- **Preserves** agent-friendly structured output when filtering

**Implementation Strategy:**
- Use `jq-rs` crate for jq syntax support with async processing
- Create transformation pipeline that processes responses before output formatting
- Ensure filters work with all output formats (JSON, table, YAML)
- Add filter validation and error handling through existing error system

### 7. Bulk Operations
**🏗️ Architectural Impact: HIGH** | **Module: `src/engine/executor.rs`, `src/batch/` (new)**

**Batch Requests:**
```bash
# Multiple entities in single command
aperture api pokeapi pokemon api_v2_pokemon_retrieve --batch "pikachu,charizard,blastoise"

# File-based input
aperture api github repos get-repo --batch-file repo-list.txt
```

**Parallel Processing:**
```bash
# Concurrent requests with rate limiting
aperture api myapi users get-user --batch user-ids.txt --parallel 5 --rate-limit 10/sec
```

**Architectural Integration:**
- **Creates** new `src/batch/` module for parallel execution management
- **Leverages** existing tokio async runtime for concurrent requests
- **Extends** `src/engine/executor.rs` with batch execution logic
- **Maintains** existing request building and authentication patterns

**Implementation Strategy:**
- Use `tokio::spawn` with `tokio::sync::Semaphore` for controlled concurrency
- Integrate rate limiting with `governor` crate for token bucket patterns
- Batch results collection and error aggregation
- Preserve individual request error handling within batch context

### 8. Configuration Profiles
**🏗️ Architectural Impact: MEDIUM** | **Module: `src/config/models.rs`, `src/config/manager.rs`**

**Environment-based Profiles:**
```bash
# Create profiles for different environments
aperture config profile create production \
  --base-url "https://api.prod.company.com" \
  --auth-token "$PROD_TOKEN"

aperture config profile create staging \
  --base-url "https://api.staging.company.com" \
  --auth-token "$STAGING_TOKEN"

# Use specific profile
aperture api --profile production myapi users list
```

**Profile Management:**
```bash
aperture config profile list
aperture config profile switch production
aperture config profile delete staging
```

**Architectural Integration:**
- **Extends** `GlobalConfig` in `src/config/models.rs` with profile support
- **Enhances** existing `ApiConfig` structure for environment-specific settings
- **Leverages** current configuration storage patterns in `~/.config/aperture/`
- **Preserves** separation of configuration and secrets

**Implementation Strategy:**
- Add `profiles` field to `GlobalConfig` with `HashMap<String, ProfileConfig>`
- Extend CLI parsing in `src/cli.rs` to accept `--profile` flag
- Profile resolution during command execution in existing config loading pipeline
- Environment variable substitution within profile context

### 9. Request/Response Caching
**🏗️ Architectural Impact: MEDIUM** | **Module: `src/cache/` (extend existing), `src/engine/executor.rs`**

**Response Caching:**
```bash
# Cache responses for development
aperture api pokeapi pokemon api_v2_pokemon_retrieve pikachu --cache 1h

# Cache management
aperture cache clear
aperture cache stats
```

**Architectural Integration:**
- **Extends** existing `src/cache/` module with response caching capabilities
- **Leverages** current binary caching patterns for consistency
- **Integrates** with existing request/response pipeline in executor
- **Maintains** cache invalidation and management through config system

**Implementation Strategy:**
- Add response cache storage alongside existing spec cache in `~/.config/aperture/.cache/`
- Use request URL + parameters as cache key with TTL support
- Cache validation based on HTTP cache headers when available
- Integration with existing file system abstraction for testability

### 10. Plugin System
**🏗️ Architectural Impact: HIGH** | **Module: `src/plugins/` (new), `src/engine/loader.rs`**

**Custom Extensions:**
```bash
# Install community plugins
aperture plugin install aperture-auth-flows
aperture plugin install aperture-testing-tools

# Custom response processors
aperture api myapi data export --processor csv-converter
```

**Architectural Integration:**
- **Creates** new `src/plugins/` module for dynamic loading
- **Extends** command generation in `src/engine/generator.rs`
- **Leverages** existing spec loading patterns for plugin discovery
- **Maintains** security boundaries and error handling

**Implementation Strategy:**
- Use `libloading` crate for dynamic library loading with safety checks
- Define plugin trait interface for command extension and response processing
- Plugin discovery through configuration directory scanning
- Sandboxed execution environment with restricted file system access

## 📋 Implementation Roadmap

### Phase 1: Core UX Improvements (Foundation)
**Target Module Integration:**
- [ ] **Parameter syntax standardization** → `src/engine/generator.rs` extension
- [ ] **Query parameter auth support** → `src/config/models.rs` + `src/engine/executor.rs`
- [ ] **Enhanced error messages** → `src/error.rs` with context-aware variants
- [ ] **Basic output formatting** → `src/engine/executor.rs` + `src/agent.rs` integration

### Phase 2: Configuration & Discovery (Enhanced Config Management)
**Target Module Integration:**
- [ ] **Remote spec support** → `src/config/url_resolver.rs` + `src/config/manager.rs`
- [ ] **Auto-discovery features** → New `src/discovery/` module
- [ ] **Configuration validation feedback** → `src/config/manager.rs` enhancement
- [ ] **Profile management** → `src/config/models.rs` extension

### Phase 3: Advanced Features (Processing Pipeline)
**Target Module Integration:**
- [ ] **Response filtering (jq integration)** → New `src/filters/` module + executor integration
- [ ] **Bulk operations** → New `src/batch/` module with tokio concurrency
- [ ] **Caching system** → `src/cache/` module extension
- [ ] **Performance optimizations** → Cross-module async improvements

### Phase 4: Ecosystem & Extensions (Plugin Architecture)
**Target Module Integration:**
- [ ] **Plugin system** → New `src/plugins/` module + loader integration
- [ ] **Community integrations** → Plugin ecosystem development
- [ ] **Advanced authentication flows** → Plugin-based auth extensions
- [ ] **Testing & monitoring tools** → Plugin-based development tooling

## 🏗️ Critical Architectural Decisions

### Design Principles Preservation
All improvements must maintain Aperture's core architectural principles:

1. **Separation of Concerns**
   - Configuration and secrets remain strictly separated
   - New features must respect `x-aperture-secret` patterns
   - Plugin system must not compromise security boundaries

2. **Agent-First Design** 
   - All new features must support `--describe-json`, `--json-errors`, `--dry-run`
   - Structured output preservation for programmatic consumption
   - Backwards compatibility for existing agent integrations

3. **Binary Caching Strategy**
   - New configuration features must integrate with existing cache invalidation
   - Remote specs cached locally following current patterns
   - Cache versioning for breaking changes in data structures

4. **Test-Driven Development**
   - All new modules require comprehensive unit + integration tests
   - Existing mock patterns extended for new functionality
   - Backwards compatibility regression testing

### Module Dependencies & Integration Points

**Core Integration Requirements:**
- `src/config/` extensions must preserve existing API and storage patterns
- `src/engine/` modifications must maintain clap Command compatibility
- `src/cache/` extensions must follow existing serialization patterns
- New modules must integrate with existing error propagation through `src/error.rs`

**Breaking Change Management:**
- Feature flags for gradual rollout of syntax changes
- Cache versioning for backwards compatibility
- Migration tooling for configuration format changes
- Deprecation warnings with clear upgrade paths

## 🧪 Testing Strategy

Each improvement should include:

1. **Unit Tests:** Core functionality validation
2. **Integration Tests:** End-to-end workflow testing
3. **Regression Tests:** Backward compatibility verification
4. **Performance Tests:** Response time and resource usage
5. **User Experience Tests:** Real-world usage scenarios

## 📖 Documentation Updates

Improvements should be accompanied by:

- Updated CLI reference documentation
- Usage examples and tutorials
- Migration guides for breaking changes
- Best practices documentation
- API integration patterns

---

**Note:** This document should be updated as features are implemented and new requirements emerge from user feedback and real-world usage patterns.