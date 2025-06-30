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

- **Bearer Token Authentication**: Uses `Authorization: Bearer <token>` header
- **API Key Authentication**: Supports header-based API keys
- **Basic Authentication**: Supports HTTP Basic auth (base64 encoded)

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

- OAuth2 and OpenID Connect are not currently supported
- Only header-based API key authentication is supported (query parameters are not supported)
- Basic authentication credentials must be provided as separate username/password environment variables

## Updates

This security policy may be updated as new features are added. Check the latest version in the repository.