# ADR-006: Partial OpenAPI Spec Acceptance with Strict Flag

## Status
Accepted

## Context
Aperture v1.0 has limitations on supported features:
1. **Content Types**: Only supports `application/json` content types for request bodies
2. **Authentication**: Only supports certain authentication schemes (Bearer, Basic, API Key, and custom HTTP schemes)

When users attempt to add OpenAPI specifications containing endpoints with unsupported features (such as `multipart/form-data` for file uploads or OAuth2 authentication), the entire specification is rejected with a validation error.

This creates a significant usability issue:
- Many real-world APIs mix supported endpoints with unsupported ones
- Users cannot use Aperture for APIs that have even a single unsupported endpoint
- The only workarounds are to manually edit specs or maintain forked versions
- APIs using OAuth2, OpenID Connect, or other complex authentication cannot be used at all

## Decision
We will implement partial OpenAPI spec acceptance that:

1. **Changes the default behavior** to accept specs with unsupported features while skipping affected endpoints
2. **Adds a `--strict` flag** to maintain the previous behavior for users who require strict validation
3. **Displays clear warnings** about skipped endpoints during `config add`
4. **Filters out unsupported endpoints** from the cached spec to prevent runtime errors
5. **Skips endpoints based on both content type and authentication requirements**

### Implementation Details

#### Validation Modes
- **Non-strict mode (default)**: Collects warnings for unsupported features but allows spec registration
- **Strict mode (`--strict` flag)**: Maintains current behavior of rejecting specs with any unsupported features

#### Warning Display
```
Warning: Skipping 2 endpoints with unsupported content types (8 of 10 endpoints will be available):
  - POST /upload (multipart/form-data) - endpoint has no supported content types
  - PUT /data (application/xml) - endpoint has no supported content types

Warning: Skipping 1 endpoints with unsupported authentication (7 of 8 endpoints will be available):
  - GET /admin - endpoint requires unsupported authentication schemes: oauth2

Use --strict to reject specs with unsupported features.
```

#### Filtering Mechanism
The `SpecTransformer` filters out endpoints flagged during validation, ensuring they are not included in the cached spec and therefore not available at runtime.

#### Content Type Matching
Content type validation is case-insensitive and ignores parameters after semicolons, following HTTP standards:
- `application/json` ✓
- `APPLICATION/JSON` ✓ 
- `Application/Json` ✓
- `application/json; charset=utf-8` ✓
- `application/json; boundary=something` ✓

This ensures maximum compatibility with real-world OpenAPI specifications that may use different content type representations.

#### Authentication Scheme Handling
Endpoints are skipped if they require ONLY unsupported authentication schemes:
- **Unsupported**: OAuth2 (all flows), OpenID Connect, HTTP Negotiate, HTTP OAuth
- **Supported**: Bearer, Basic, API Key, and any custom HTTP schemes (Token, DSN, etc.)

If an endpoint has multiple authentication options (OR relationship) and at least one is supported, the endpoint remains available:
```yaml
security:
  - oauth2: []      # Unsupported
  - bearerAuth: []  # Supported - endpoint is available
```

## Consequences

### Positive
- **Improved user experience**: Users can work with APIs that have mixed endpoint support
- **Progressive enhancement**: As new content types are supported, endpoints automatically become available
- **Clear communication**: Users understand exactly what's not supported and why
- **Backward compatibility**: The `--strict` flag preserves the original behavior for those who need it

### Negative
- **Breaking change**: Default behavior changes (though mitigated since we have no users yet)
- **Potential confusion**: Users might expect all endpoints to work if the spec is accepted
- **Incomplete API coverage**: Some functionality will be silently unavailable

### Neutral
- **Additional complexity**: The codebase now handles two validation modes
- **Warning fatigue**: Users might ignore warnings if they appear frequently

## Alternative Approaches Considered

1. **Maintain strict-only behavior**: Rejected as too restrictive for practical use
2. **Support all content types**: Out of scope for v1.0 due to complexity
3. **Interactive mode**: Ask users during `config add` - rejected to maintain non-interactive CLI design
4. **Separate command**: Having `config add-partial` - rejected as unnecessarily complex

## References
- Issue #11: Original feature request and discussion
- PR #12: Implementation of this decision
- Architecture doc: Updated to reflect new validation modes