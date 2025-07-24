# Security Policy

## Reporting Security Vulnerabilities

If you discover a security vulnerability in Aperture, please report it privately by emailing the maintainer. Please do not create public GitHub issues for security vulnerabilities.

## Security Model

### Authentication

Aperture uses a secure environment variable-based authentication system that maintains strict separation between configuration and secrets:

#### x-aperture-secret Extensions

Authentication is configured through custom `x-aperture-secret` extensions in OpenAPI specifications:

```yaml
components:
  securitySchemes:
    apiToken:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
```

#### Supported Authentication Methods

1. **API Key Authentication**: Supports header, query, or cookie-based API keys
   ```yaml
   apiKey:
     type: apiKey
     in: header  # or 'query' or 'cookie'
     name: X-API-Key
   ```

2. **HTTP Bearer Token**: Uses `Authorization: Bearer <token>` header
   ```yaml
   bearerAuth:
     type: http
     scheme: bearer
   ```

3. **HTTP Basic Authentication**: Uses `Authorization: Basic <base64>` header
   ```yaml
   basicAuth:
     type: http
     scheme: basic
     x-aperture-secret:
       source: env
       name: BASIC_CREDS  # Format: username:password (base64 encoding handled automatically)
   ```

4. **Custom HTTP Schemes**: Any HTTP scheme not explicitly rejected
   ```yaml
   # Examples: Token, DSN, ApiKey, X-Custom-Auth, etc.
   tokenAuth:
     type: http
     scheme: Token  # Results in: Authorization: Token <token>
   ```

All custom HTTP schemes are treated as bearer-like tokens with the format: `Authorization: <scheme> <token>`

#### Security Features

1. **Environment Variable Storage**: Secrets are never stored in configuration files
2. **Runtime Resolution**: Secrets are resolved from environment variables at request time
3. **Clear Error Messages**: Missing environment variables produce actionable error messages
4. **No Secret Logging**: Secrets are never logged or exposed in debug output

### Custom Headers

The `--header` flag supports environment variable expansion for secure header injection:

```bash
aperture api myapi operation --header "X-Custom-Token: ${MY_SECRET_TOKEN}"
```

### Global Security Inheritance

Aperture properly implements OpenAPI 3.0 global security inheritance, applying spec-level security requirements to operations that don't define their own security.

## Best Practices

1. **Never commit secrets**: Use environment variables for all authentication credentials
2. **Use descriptive environment variable names**: Make it clear which API and purpose each variable serves
3. **Rotate credentials regularly**: Update environment variables containing API keys and tokens
4. **Scope permissions**: Use the minimum required permissions for API credentials
5. **Test with dry-run**: Use `--dry-run` to verify request structure before execution

## Limitations

The following authentication types are explicitly not supported due to their complexity:

- **OAuth2** (all flows) - Requires token management, refresh flows, and state persistence
- **OpenID Connect** - Even more complex than OAuth2 with discovery endpoints
- **HTTP Negotiate** (Kerberos/NTLM) - Requires complex authentication handshakes
- **HTTP OAuth scheme** - Indicates OAuth 1.0 which requires request signing

For APIs using these authentication methods, consider using alternative authentication schemes if available (e.g., API tokens or personal access tokens).

### Partial API Support

Starting from v0.1.4, Aperture uses a non-strict validation mode by default:
- APIs containing unsupported authentication schemes are accepted
- Only endpoints that require unsupported authentication are skipped
- Endpoints with multiple authentication options (where at least one is supported) remain available
- Use the `--strict` flag with `aperture config add` to reject specs with any unsupported features

This allows you to use most endpoints of an API even if some require unsupported authentication methods.

## Updates

This security policy may be updated as new features are added. Check the latest version in the repository.