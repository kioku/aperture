# Security Model

Aperture enforces strict separation between configuration and secrets. API specifications are stored as configuration files; credentials are always resolved from environment variables at runtime.

## Core Principles

1. **Secrets never touch disk**: Credentials are read from environment variables, never stored in config files
2. **Explicit mapping**: Each authentication scheme maps to a named environment variable
3. **Fail-safe**: Missing credentials produce clear errors, not silent failures
4. **Auditable**: Configuration files can be safely committed to version control

## Authentication Methods

### API Key

API keys sent in headers, query parameters, or cookies.

**OpenAPI spec:**

```yaml
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header           # or: query, cookie
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

## Unsupported Authentication

The following require complex flows and are not supported:

| Type | Reason |
|------|--------|
| OAuth2 (all flows) | Requires browser interaction, token refresh |
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
# Warning: Skipping 3 endpoints requiring OAuth2 authentication
# Added my-api with 47 available commands
```

### Strict Mode

Reject specs containing any unsupported features:

```bash
aperture config add --strict my-api ./openapi.yaml
# Error: Specification contains unsupported authentication: oauth2
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

When caching is enabled (`--cache`) and the request includes authentication headers, Aperture skips caching entirely and makes a fresh request every time. This is a deliberate security default.

### Authentication Header Scrubbing

As an additional defense-in-depth measure, authentication headers are scrubbed from any request metadata that does get stored in the cache. The following headers are automatically removed:

- `Authorization` / `Proxy-Authorization`
- `X-API-Key` / `X-API-Token` / `API-Key`
- `Cookie` (may contain session tokens)

This ensures that even if cache files are accessed by other processes or backed up, no credentials are exposed.

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
