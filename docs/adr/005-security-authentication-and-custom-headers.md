# ADR-005: Security Authentication and Custom Headers Implementation

## Status
Accepted

## Context
When implementing Phase 6 of Aperture (Security and Custom Headers), we needed to address critical gaps that prevented real-world API usage:

1. **No Authentication Support**: The executor had TODO placeholders for x-aperture-secret authentication but no actual implementation
2. **Missing Custom Headers**: Users had no way to add operational headers like request IDs, tracing headers, or debugging information
3. **Agent Discovery Gap**: The `--describe-json` capability manifest didn't expose security requirements to autonomous agents
4. **Security Separation**: Need to maintain strict separation between OpenAPI specs (configuration) and credentials (secrets)

The solution needed to support common authentication schemes (Bearer tokens, API keys, Basic auth) while maintaining backward compatibility and enabling agent automation.

## Decision
We implemented a comprehensive security and custom headers system with four key components:

### 1. Enhanced Cache Models
Extended the cached spec representation to include security information:

```rust
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CachedSecurityScheme {
    pub name: String,
    pub scheme_type: String,          // "http", "apiKey", etc.
    pub scheme: Option<String>,       // "bearer", "basic", etc.  
    pub location: Option<String>,     // "header", "query", "cookie"
    pub parameter_name: Option<String>, // Header/parameter name
    pub aperture_secret: Option<CachedApertureSecret>,
}
```

### 2. Environment Variable Authentication
Implemented secure credential resolution in the executor:
- Bearer tokens: `Authorization: Bearer ${TOKEN}`
- API keys: Custom header names (e.g., `X-API-Key: ${KEY}`)
- Basic auth: `Authorization: Basic ${CREDENTIALS}`
- Clear error messages when environment variables are missing

### 3. Custom Headers with --header/-H Flag
Added custom header support to all operation commands:
- Format: `--header "Name: Value"` or `-H "Name: Value"`
- Environment variable expansion: `--header "X-Trace-ID: ${TRACE_ID}"`
- Multiple headers supported via repeated flags
- Comprehensive validation and error handling

### 4. Agent Capability Manifest Enhancement
Updated the agent manifest to expose security information:
- Security scheme types and details
- Environment variable names required
- Available authentication schemes summary

## Consequences

### Positive
- **Real-World API Usage**: Proper authentication enables production API integration
- **Agent Automation**: Autonomous agents can discover and configure authentication
- **Operational Support**: Custom headers enable tracing, debugging, and monitoring
- **Backward Compatibility**: All existing functionality continues unchanged
- **Security Best Practices**: Environment variables prevent credential exposure

### Negative
- **Environment Variable Management**: Users must manage authentication credentials
- **Test Complexity**: Integration tests require careful environment variable isolation
- **Parallel Test Limitations**: Security tests must run single-threaded (`--test-threads=1`)

### Neutral
- **Phase-Based Implementation**: Six atomic phases maintained system stability during development
- **Comprehensive Testing**: 8 integration tests cover all authentication and header scenarios

## Alternatives Considered

1. **Configuration File Secrets**: Rejected due to security risks and credential exposure
2. **Command-Line Secret Parameters**: Rejected due to shell history and process list visibility  
3. **Interactive Prompts**: Rejected due to incompatibility with agent automation
4. **External Secret Managers**: Deferred for v2.0 to maintain v1.0 simplicity

## Future Enhancements
1. OAuth2 flow support with refresh tokens
2. External secret manager integration (Vault, AWS Secrets Manager)
3. Certificate-based authentication
4. Advanced security scheme validation against OpenAPI specs