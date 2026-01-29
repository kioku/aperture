# Implementation Plan: Request/Response Logging for Debugging (Issue #58)

## Executive Summary

This plan introduces structured request/response logging to Aperture using the `tracing` ecosystem. The implementation spans 5 phases, introducing environment variable controls, CLI flags, multi-level logging with security redaction, and flexible output destinations.

---

## Requirements Summary

### Environment Controls
- `APERTURE_LOG` - Log levels: error, warn, info, debug, trace
- `APERTURE_LOG_FILE` - Output to file instead of stderr
- `APERTURE_LOG_FORMAT` - Output format: text (default) or json
- `APERTURE_LOG_MAX_BODY` - Body truncation limit (default: 1000 chars)
- `APERTURE_LOG_REDACT` - Custom patterns to redact (comma-separated)

### CLI Flags
- `-v` / `--verbose` - Enable debug level logging
- `-vv` - Enable trace level logging

### Log Levels
| Level | Output |
|-------|--------|
| info | method, URL, status, duration |
| debug | + request/response headers (redacted) |
| trace | + request/response bodies (truncated) |

---

## Phase 1: Dependencies and Foundation

**Complexity: Low | ~100 LOC**

### 1.1 Add Dependencies

**File: `Cargo.toml`**

```toml
[dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json", "fmt"] }
```

### 1.2 Create Logging Module Structure

**New Directory: `src/logging/`**

```
src/logging/
├── mod.rs          # Module exports
├── config.rs       # Configuration parsing
├── redaction.rs    # Sensitive data masking
└── subscriber.rs   # Tracing subscriber setup
```

**File: `src/lib.rs`** - Add module declaration:
```rust
pub mod logging;
```

### 1.3 Add Environment Variable Constants

**File: `src/constants.rs`** - Add:

```rust
// Logging Environment Variables
pub const ENV_APERTURE_LOG: &str = "APERTURE_LOG";
pub const ENV_APERTURE_LOG_FILE: &str = "APERTURE_LOG_FILE";
pub const ENV_APERTURE_LOG_FORMAT: &str = "APERTURE_LOG_FORMAT";
pub const ENV_APERTURE_LOG_MAX_BODY: &str = "APERTURE_LOG_MAX_BODY";
pub const ENV_APERTURE_LOG_REDACT: &str = "APERTURE_LOG_REDACT";

// Defaults
pub const DEFAULT_LOG_MAX_BODY: usize = 1000;
```

---

## Phase 2: CLI Flag Integration

**Complexity: Low-Medium | ~150 LOC**

### 2.1 Add Verbose Flag to CLI

**File: `src/cli.rs`** - Add to `Cli` struct (global flags section):

```rust
/// Enable verbose logging output (debug level)
/// Use -v for debug, -vv for trace level
#[arg(
    short = 'v',
    long = "verbose",
    global = true,
    action = clap::ArgAction::Count,
    help = "Increase logging verbosity (-v debug, -vv trace)"
)]
pub verbose: u8,
```

### 2.2 Implement Logging Configuration

**File: `src/logging/config.rs`**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub level: LogLevel,
    pub format: LogFormat,
    pub file_path: Option<PathBuf>,
    pub max_body_length: usize,
    pub custom_redact_patterns: Vec<String>,
}

impl LogConfig {
    /// Build from environment variables and CLI flags
    /// Priority: CLI flags > environment variables > defaults
    pub fn from_env_and_cli(verbose_count: u8) -> Self { ... }
}
```

---

## Phase 3: Subscriber Initialization

**Complexity: Medium | ~200 LOC**

### 3.1 Implement Subscriber Setup

**File: `src/logging/subscriber.rs`**

Key functionality:
- Build `EnvFilter` from resolved log level
- Configure output format (text or JSON)
- Configure output destination (stderr or file)
- Initialize global subscriber

```rust
pub fn init_logging(config: &LogConfig) -> Result<(), Box<dyn std::error::Error>> {
    let filter = build_env_filter(config.level);

    match (config.format, &config.file_path) {
        (LogFormat::Json, None) => { /* JSON to stderr */ }
        (LogFormat::Json, Some(path)) => { /* JSON to file */ }
        (LogFormat::Text, None) => { /* Text to stderr */ }
        (LogFormat::Text, Some(path)) => { /* Text to file */ }
    }

    Ok(())
}
```

### 3.2 Initialize in Main

**File: `src/main.rs`** - Add early in `main()`:

```rust
let log_config = aperture_cli::logging::config::LogConfig::from_env_and_cli(cli.verbose);
if let Err(e) = aperture_cli::logging::subscriber::init_logging(&log_config) {
    eprintln!("Warning: Failed to initialize logging: {e}");
}
```

---

## Phase 4: Request/Response Instrumentation

**Complexity: High | ~400 LOC**

### 4.1 Implement Redaction Logic

**File: `src/logging/redaction.rs`**

```rust
pub struct Redactor {
    sensitive_headers: HashSet<String>,
    custom_patterns: Vec<String>,
}

impl Redactor {
    /// Headers always redacted (non-configurable)
    fn default_sensitive_headers() -> HashSet<String> {
        ["authorization", "proxy-authorization", "x-api-key",
         "x-api-token", "x-auth-token", "cookie", "set-cookie"]
            .iter().map(|s| s.to_lowercase()).collect()
    }

    pub fn is_sensitive_header(&self, name: &str) -> bool { ... }
    pub fn redact_headers(&self, headers: &HeaderMap) -> Vec<(String, String)> { ... }
    pub fn truncate_body(&self, body: &str, max_length: usize) -> String { ... }
}
```

### 4.2 Instrument Executor Functions

**File: `src/engine/executor.rs`**

Key modifications:

#### Add imports:
```rust
use tracing::{debug, error, info, trace, warn, instrument, Span};
```

#### Instrument `execute_request` (~line 595):
```rust
#[instrument(
    name = "http_request",
    skip(spec, matches, global_config, cache_config, retry_context),
    fields(api = %spec.name, operation_id, method, url, status, duration_ms)
)]
pub async fn execute_request(...) -> Result<Option<String>, Error> {
    info!(method = %method, url = %url, "Sending request");
    // ... existing logic ...
}
```

#### Modify `send_request` (~line 210):
```rust
async fn send_request(...) -> Result<(...), Error> {
    let start = std::time::Instant::now();
    let response = request.send().await?;
    let duration = start.elapsed();

    info!(status = status.as_u16(), duration_ms = duration.as_millis(), "Response received");
    debug!(headers = ?redactor.redact_headers(&response_headers), "Response headers");
    trace!(body = %redactor.truncate_body(&response_text, max_body), "Response body");
    // ...
}
```

#### Modify `build_request` (~line 416):
```rust
fn build_request(...) -> reqwest::RequestBuilder {
    debug!(headers = ?redactor.redact_headers(&headers), "Request headers");
    if let Some(ref body) = body {
        trace!(body = %redactor.truncate_body(body, max_body), "Request body");
    }
    // ...
}
```

#### Replace retry logging (~line 323):
```rust
// Replace eprintln! with:
info!(
    attempt = %attempt,
    max_attempts = %max_attempts,
    status = %status.as_u16(),
    delay_ms = %delay.as_millis(),
    "Retrying request"
);
```

#### Replace auth logging (~line 1168):
```rust
// Replace RUST_LOG checks with:
debug!(scheme_name = %scheme.name, scheme_type = %scheme.scheme_type, "Adding auth header");
```

---

## Phase 5: Testing Strategy

**Complexity: Medium | ~500 LOC**

### 5.1 Unit Tests

**File: `tests/logging_redaction_tests.rs`**
- Test sensitive header detection
- Test custom pattern matching
- Test body truncation
- Test body content redaction

**File: `tests/logging_config_tests.rs`**
- Test CLI verbose flag precedence
- Test environment variable parsing
- Test default values

### 5.2 Integration Tests

**File: `tests/logging_integration_tests.rs`**
- Test `-v` flag enables debug output
- Test `-vv` flag enables trace output
- Test `APERTURE_LOG_FILE` writes to file
- Test `APERTURE_LOG_FORMAT=json` produces valid JSON

**File: `tests/logging_http_tests.rs`**
- Test actual HTTP request/response logging with wiremock
- Verify sensitive headers are redacted in output
- Verify body truncation works correctly

---

## Security Considerations

### Always Redacted Headers (Non-Configurable)
- `Authorization`, `Proxy-Authorization`
- `X-Api-Key`, `X-Api-Token`, `X-Auth-Token`
- `Cookie`, `Set-Cookie`
- Headers starting with `x-auth-` or `x-api-`

### Custom Redaction
- `APERTURE_LOG_REDACT=api-key,password,secret` for additional patterns
- Applies to both headers and body content

### Body Truncation
- Default: 1000 characters
- Configurable via `APERTURE_LOG_MAX_BODY`
- Prevents accidental logging of large sensitive payloads

---

## Backward Compatibility

| Existing Feature | Impact |
|-----------------|--------|
| `RUST_LOG` env var | Replaced by `APERTURE_LOG`, but `tracing-subscriber` respects `RUST_LOG` as fallback |
| `--quiet` mode | Unaffected - logging goes to stderr, quiet suppresses stdout |
| `--json-errors` | Unaffected - controls error format, not logging |
| `eprintln!` debug output | Migrated to `tracing` macros |

---

## Files Summary

| File | Action | Phase |
|------|--------|-------|
| `Cargo.toml` | Modify - add deps | 1 |
| `src/lib.rs` | Modify - add module | 1 |
| `src/constants.rs` | Modify - add constants | 1 |
| `src/logging/mod.rs` | Create | 1 |
| `src/logging/config.rs` | Create | 2 |
| `src/logging/subscriber.rs` | Create | 3 |
| `src/logging/redaction.rs` | Create | 4 |
| `src/cli.rs` | Modify - add verbose flag | 2 |
| `src/main.rs` | Modify - init logging | 3 |
| `src/engine/executor.rs` | Modify - instrument | 4 |
| `tests/logging_*.rs` | Create (4 files) | 5 |

---

## Acceptance Criteria Checklist

- [ ] `APERTURE_LOG` environment variable works with 5 levels
- [ ] `-v` flag enables debug level
- [ ] `-vv` flag enables trace level
- [ ] Info level logs: method, URL, status, duration
- [ ] Debug level adds: request/response headers
- [ ] Trace level adds: request/response bodies (truncated)
- [ ] `Authorization` headers are redacted
- [ ] `APERTURE_LOG_REDACT` supports custom patterns
- [ ] `APERTURE_LOG_MAX_BODY` controls truncation
- [ ] `APERTURE_LOG_FILE` writes to file
- [ ] `APERTURE_LOG_FORMAT=json` produces structured logs

---

## Dependency Impact

| Crate | Size Impact | Purpose |
|-------|-------------|---------|
| `tracing` | ~50KB | Core instrumentation API |
| `tracing-subscriber` | ~150KB | Formatting, filtering, output |
| **Total** | ~200KB | Minimal impact |
