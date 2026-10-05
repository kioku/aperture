# Security Model

Aperture enforces strict separation between configuration and secrets. API specifications are stored as configuration files; credentials are always resolved from environment variables at runtime.

## Core Principles

1. **Secrets never touch disk**: Credentials are read from environment variables, never stored in config files
2. **Explicit mapping**: Each authentication scheme maps to a named environment variable
3. **Fail-safe**: Missing credentials produce clear errors, not silent failures
4. **Auditable**: Configuration files can be safely committed to version control

## Authentication Methods

### API Key

API keys are sent in headers. Query and cookie security schemes are not injected automatically and fail explicitly rather than sending an unauthenticated request.

**OpenAPI spec:**

```yaml
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: MY_API_KEY
```

**Environment:**

```bash
export MY_API_KEY="your-api-key-here"
```

### HTTP Bearer Token

JWT tokens or other bearer authentication.

**OpenAPI spec:**

```yaml
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
```

**Environment:**

```bash
export API_TOKEN="eyJhbGciOiJIUzI1NiIs..."
```

### HTTP Basic Authentication

Username and password authentication.

**OpenAPI spec:**

```yaml
components:
  securitySchemes:
    basicAuth:
      type: http
      scheme: basic
      x-aperture-secret:
        source: env
        name: BASIC_CREDENTIALS
```

**Environment:**

```bash
# Format: username:password (base64 encoding is automatic)
export BASIC_CREDENTIALS="admin:secretpassword"
```

### Custom HTTP Schemes

Non-standard schemes like Token, DSN, or proprietary formats.

**OpenAPI spec:**

```yaml
components:
  securitySchemes:
    # Token scheme (alternative to Bearer)
    tokenAuth:
      type: http
      scheme: Token
      x-aperture-secret:
        source: env
        name: API_TOKEN

    # Sentry-style DSN
    dsnAuth:
      type: http
      scheme: DSN
      x-aperture-secret:
        source: env
        name: SENTRY_DSN

    # Proprietary scheme
    customAuth:
      type: http
      scheme: X-CompanyAuth-V2
      x-aperture-secret:
        source: env
        name: COMPANY_TOKEN
```

All custom HTTP schemes are formatted as: `Authorization: <scheme> <token>`

## Dynamic Secret Configuration

Configure authentication without modifying OpenAPI specs—useful for third-party APIs.

### CLI Commands

```bash
# Map a security scheme to an environment variable
aperture config set-secret my-api bearerAuth --env API_TOKEN

# Interactive configuration (lists available schemes)
aperture config set-secret my-api --interactive

# List configured secrets
aperture config list-secrets my-api
```

### Priority Order

1. **CLI-configured secrets** (highest priority)
2. **x-aperture-secret extensions** in OpenAPI spec
3. **Error** if neither is configured

This allows overriding spec-defined mappings without editing the spec.

## OAuth2 external access tokens

OpenAPI `type: oauth2` schemes are supported using access tokens obtained outside
Aperture. Keep the original OAuth2 declaration and map its scheme name to an
environment variable, either with `x-aperture-secret` or a configured override:

```bash
aperture config secret set my-api oauth2 --env MY_API_ACCESS_TOKEN
```

Acquire and update `MY_API_ACCESS_TOKEN` using your provider's tools. Aperture
reads it at invocation time and sends `Authorization: Bearer <token>` to the API.
Configured mappings override extensions. Token values are not stored in config
or parsed-spec caches; authenticated requests bypass the response cache.

Discovery retains declared flows and operation scope requirements. These describe
the API contract, not verified grants: Aperture does not inspect opaque tokens,
verify JWTs, or validate expiry or granted scopes. `--describe-json` reports
`execution_mode: "externalBearerToken"` and `token_grants_verified: false` for
OAuth2 scheme details, plus `security_scopes` aligned with security alternatives.

Aperture performs no login, grant exchange, authorization URL navigation, token
endpoint requests, refresh, or token persistence. Missing or invalid credentials
fail before execution. API authentication failures are reported under the existing
retry policy; they never trigger token acquisition or refresh. Replace an expired
or rejected token externally before invoking the API again. Registration,
discovery, and dry-run never contact the declared OAuth2 flow URLs.

## Unsupported Authentication

The following require complex flows and are not supported:

| Type | Reason |
|------|--------|
| OAuth2 token acquisition/refresh | Use external provider tools |
| OpenID Connect | Requires discovery, token management |
| HTTP Negotiate | Kerberos/NTLM require system integration |
| Mutual TLS | Certificate management out of scope |

## Partial API Support

APIs with mixed authentication methods are handled gracefully.

### Default Mode (Non-Strict)

Aperture accepts specs with unsupported features:
- Endpoints requiring unsupported auth are skipped
- Endpoints with multiple auth options (where one is supported) remain available
- Warnings indicate which endpoints are skipped and why

```bash
aperture config add my-api ./openapi.yaml
# Warning: Skipping 3 endpoints requiring OpenID Connect authentication
# Added my-api with 47 available commands
```

### Strict Mode

Reject specs containing any unsupported features:

```bash
aperture config add --strict my-api ./openapi.yaml
# Error: Specification contains unsupported authentication: openIdConnect
```

## The x-aperture-secret Extension

This OpenAPI extension maps security schemes to environment variables.

**Schema:**

```yaml
x-aperture-secret:
  source: env        # Currently only "env" is supported
  name: <VAR_NAME>   # Environment variable name
```

**Placement:**

Add to any security scheme in `components/securitySchemes`:

```yaml
components:
  securitySchemes:
    myAuth:
      type: http
      scheme: bearer
      x-aperture-secret:      # <-- Extension here
        source: env
        name: MY_TOKEN
```

## Response Cache Security

The response cache system is designed to prevent credential leakage to disk.

### Default Behavior: Skip Authenticated Requests

Responses from authenticated requests are **not cached**. This prevents authorization headers and tokens from being persisted in the file-based response cache.

```bash
# This request uses a Bearer token — response is NOT cached
aperture api my-api --cache users list
```

When caching is enabled (`--cache`), Aperture skips caching for operations with active security requirements (including custom header, query, and cookie API keys), and for requests carrying known authentication headers or cookies. This conservative policy also applies when SDK callers set the legacy `CacheConfig.allow_authenticated` flag: authenticated caching is disabled. No account-specific cached response is reused.

Existing cache files are not rewritten. If you used authenticated caching with an earlier version, run `aperture config cache clear --all` to remove previously persisted credentials or session cookies. This clears all response caches; API-specific clearing uses the new escaped filename prefix and may not match older files for names containing underscores or dots.

### Authentication Header Scrubbing

As an additional defense-in-depth measure, authentication headers are scrubbed from any request metadata that does get stored in the cache. The following headers are automatically removed:

- `Authorization` / `Proxy-Authorization`
- `Cookie` and response `Set-Cookie`
- `X-API-Key` / `X-API-Token` / `API-Key`

Executor requests with repeated header fields bypass caching because the cache key input cannot represent multiple values for one header. New cache entries omit all request-header metadata and strip URL userinfo, query strings, and fragments. The low-level SDK cache API has no OpenAPI security context: its request keys include every supplied header in a length-framed SHA-256 digest to separate credential identities. Callers must supply the complete request when generating keys and must not put secrets in response bodies or unrecognized response headers. Both executor and low-level cache storage skip responses with `Set-Cookie` and unsafe request methods.

Enabled request logging redacts URL userinfo and sensitive query parameters, including percent-encoded parameter names and custom query API keys declared by the operation. Disabled logging still avoids URL, header, and body redaction work.

Cache filenames escape caller-supplied components, including path separators and Unicode. The new key encoding intentionally invalidates previous request-key matches; existing files remain on disk until cleared or expired.

### Context Name Validation

API context names are validated to prevent path traversal attacks. Names containing `..`, `/`, `\`, or other path-separator characters are rejected. This ensures that API context names cannot be used to write cache or configuration files outside the expected directory.

## Best Practices

### 1. Use Descriptive Variable Names

```bash
# Good: Clear which API and purpose
export GITHUB_API_TOKEN="..."
export STRIPE_SECRET_KEY="..."

# Avoid: Ambiguous
export TOKEN="..."
export KEY="..."
```

### 2. Separate Environments

```bash
# Development
export MYAPI_TOKEN="dev-token"

# Production (different shell/environment)
export MYAPI_TOKEN="prod-token"
```

Or use Aperture's environment-specific URL configuration:

```bash
aperture config set-url my-api --env dev https://dev.api.example.com
aperture config set-url my-api --env prod https://api.example.com

APERTURE_ENV=prod aperture api my-api users list
```

### 3. Avoid Committing Secrets

The configuration structure is safe to commit:

```
~/.config/aperture/
├── specs/my-api.yaml     # Safe: No secrets
├── config.toml           # Safe: Only references env var names
└── .cache/               # Safe: Binary cache, no secrets
```

### 4. Rotate Credentials

Update environment variables without changing configuration:

```bash
# Old credential
export API_TOKEN="old-token"

# Rotate to new credential
export API_TOKEN="new-token"

# Aperture picks up new value immediately
aperture api my-api users list
```

### 5. Use Secret Management Tools

Integrate with secret managers:

```bash
# AWS Secrets Manager
export API_TOKEN=$(aws secretsmanager get-secret-value --secret-id my-api-token --query SecretString --output text)

# HashiCorp Vault
export API_TOKEN=$(vault kv get -field=token secret/my-api)

# 1Password CLI
export API_TOKEN=$(op read "op://Vault/MyAPI/token")
```

## Error Messages

Clear errors when authentication fails:

```
Authentication: Environment variable 'API_TOKEN' is not set

Hint: Set the environment variable before retrying:
  export API_TOKEN="your-token-here"
```

With `--json-errors`:

```json
{
  "error_type": "Authentication",
  "message": "Environment variable 'API_TOKEN' is not set",
  "details": {
    "scheme_name": "bearerAuth",
    "env_var": "API_TOKEN"
  }
}
```

External OAuth2 tokens must satisfy RFC 6750 bearer-token syntax: nonempty ASCII
letters, digits, `-._~+/`, optionally followed by `=` padding. Spaces and control
characters are rejected before dry-run or network execution; this does not verify
expiry or grants. An unusable OAuth2 credential makes its complete security
alternative unavailable. In non-strict registration, complete alternatives that
require unsupported OpenID Connect are omitted when another supported alternative
exists; no member of an AND requirement is removed individually.
If a specification was already registered with unsupported alternatives before
this correction, run `aperture config reinit` to rebuild its parsed-spec cache.
Unknown scheme references in cached commands remain errors rather than being
silently treated as anonymous access.
