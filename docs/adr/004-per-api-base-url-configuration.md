# ADR-004: Per-API Base URL Configuration

## Status
Accepted

## Context
Users needed a way to configure different base URLs for different APIs and environments, beyond the simple global `APERTURE_BASE_URL` environment variable. This was particularly important for:

1. **Multi-environment deployments**: Different APIs in staging vs production
2. **Local development**: Overriding production URLs for testing
3. **Per-API customization**: Different base URLs for different APIs in the same environment
4. **Agent-friendly workflows**: Programmatic configuration management

The original design only supported a global `APERTURE_BASE_URL` environment variable, which was insufficient for complex deployment scenarios.

## Decision
We implemented a comprehensive base URL management system with the following components:

### 1. Enhanced Data Models
- `ApiConfig` struct for per-API configuration
- `GlobalConfig` with `api_configs` HashMap for multiple APIs
- `CachedSpec` enhanced with extracted server URLs from OpenAPI specs

### 2. URL Resolution Priority Hierarchy
1. **Explicit parameter** (for testing)
2. **Per-API config override** with environment support
3. **APERTURE_BASE_URL** environment variable (global)
4. **Cached spec default** (extracted from OpenAPI servers)
5. **Fallback URL** (`https://api.example.com`)

### 3. CLI Management Commands
- `aperture config set-url <api> <url>` - Set base override
- `aperture config set-url <api> --env <env> <url>` - Set environment-specific URL
- `aperture config get-url <api>` - View configuration
- `aperture config list-urls` - View all configurations

### 4. Environment Variable Support
- `APERTURE_ENV` for selecting environment-specific URLs
- Backward compatibility with existing `APERTURE_BASE_URL` usage

## Implementation Details

### BaseUrlResolver
```rust
pub struct BaseUrlResolver<'a> {
    spec: &'a CachedSpec,
    global_config: Option<&'a GlobalConfig>,
    environment_override: Option<String>,
}
```

The resolver implements the priority hierarchy and provides a clean abstraction for URL resolution across the application.

### Configuration Storage
```toml
# ~/.config/aperture/config.toml
[api_configs.my-api]
base_url_override = "https://custom.example.com"

[api_configs.my-api.environment_urls]
staging = "https://staging.example.com"
prod = "https://prod.example.com"
```

### Automatic Server URL Extraction
During `aperture config add`, the system:
1. Parses the OpenAPI spec's `servers` array
2. Extracts the first server URL as the default base URL
3. Stores all server URLs for future multi-environment support
4. Caches this information in the binary cache file

## Alternatives Considered

### 1. Global Configuration Only
- **Pros**: Simple implementation
- **Cons**: Inflexible, doesn't support per-API customization
- **Rejected**: Insufficient for real-world use cases

### 2. Environment Variables Per API
- **Pros**: Familiar pattern
- **Cons**: Pollutes environment namespace, difficult to manage
- **Rejected**: Poor user experience with many APIs

### 3. YAML Configuration Files
- **Pros**: Rich configuration format
- **Cons**: More complex parsing, validation overhead
- **Rejected**: TOML provides sufficient functionality with simplicity

### 4. Command-Line Arguments Only
- **Pros**: Explicit configuration
- **Cons**: Verbose, not persistent
- **Rejected**: Poor user experience for common operations

## Consequences

### Positive
- **Flexible Configuration**: Supports complex deployment scenarios
- **Backward Compatibility**: Existing usage patterns continue to work
- **Environment Support**: Easy switching between staging/production
- **Agent-Friendly**: Programmatic configuration management
- **Automatic Extraction**: Base URLs extracted from OpenAPI specs
- **Clear Priority**: Well-defined resolution hierarchy

### Negative
- **Complexity**: More configuration options to understand
- **Storage**: Additional configuration files to manage

### Neutral
- **Migration**: No breaking changes to existing workflows
- **Performance**: Minimal overhead during URL resolution

## Testing Strategy

The implementation includes comprehensive testing:

1. **Unit Tests**: BaseUrlResolver priority hierarchy verification
2. **Integration Tests**: End-to-end CLI command testing
3. **Backward Compatibility Tests**: Existing workflows continue to work
4. **Environment Isolation**: Thread-safe test execution

## Future Enhancements

This design enables future features:
- Multiple server URL support from OpenAPI specs
- Regional URL routing
- Load balancing configuration
- Dynamic URL discovery

## References
- ADR-003: Test Isolation and Base URL Configuration (superseded)
- OpenAPI 3.x Server Object specification
- Conventional Commits for atomic implementation