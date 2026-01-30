# Debugging Guide: Request/Response Logging

This guide explains how to use Aperture's built-in logging capabilities to debug and troubleshoot API interactions.

## Quick Start

Enable debug logging with the `-v` flag:

```bash
aperture -v api myapi users get-user --id 123
```

For more detailed logging including response bodies, use `-vv`:

```bash
aperture -vv api myapi users get-user --id 123
```

## Log Levels

Aperture supports five log levels that can be controlled via the `APERTURE_LOG` environment variable:

### Error (default)
Only error messages are logged. This is the default behavior.

```bash
APERTURE_LOG=error aperture api myapi users get-user --id 123
```

### Warn
Warnings and errors are logged.

```bash
APERTURE_LOG=warn aperture api myapi users get-user --id 123
```

### Info
Shows request/response summaries with method, URL, status, and duration.

```bash
APERTURE_LOG=info aperture api myapi users get-user --id 123
```

Output example:
```
→ GET https://api.example.com/users/123
← 200 OK (143ms)
```

### Debug
Includes request and response headers in addition to info-level logging.

```bash
APERTURE_LOG=debug aperture api myapi users get-user --id 123
aperture -v api myapi users get-user --id 123  # Equivalent
```

Output example:
```
→ GET https://api.example.com/users/123
Request headers:
  Content-Type: application/json
  User-Agent: aperture/0.1.8
  Authorization: [REDACTED]
← 200 OK (143ms)
Response headers:
  Content-Type: application/json
  X-Request-Id: abc-123
```

### Trace
Includes request and response bodies (with automatic truncation). Provides maximum verbosity.

```bash
APERTURE_LOG=trace aperture api myapi users get-user --id 123
aperture -vv api myapi users get-user --id 123  # Equivalent
```

Output example:
```
→ GET https://api.example.com/users/123
Request headers:
  Authorization: [REDACTED]
Request body: (none)
← 200 OK (143ms)
Response headers:
  Content-Type: application/json
Response body: {"id": "123", "name": "Alice", ...} (truncated at 1000 chars)
```

## CLI Flags

### `-v` (Single Verbosity)
Enables debug-level logging (equivalent to `APERTURE_LOG=debug`).

```bash
aperture -v api myapi users get-user --id 123
```

### `-vv` (Double Verbosity)
Enables trace-level logging (equivalent to `APERTURE_LOG=trace`), including request/response bodies.

```bash
aperture -vv api myapi users get-user --id 123
```

## Environment Variables

### `APERTURE_LOG`
Sets the logging level. Valid values: `error`, `warn`, `info`, `debug`, `trace`.

```bash
APERTURE_LOG=debug aperture api myapi users get-user --id 123
```

### `APERTURE_LOG_FORMAT`
Sets the output format for logs. Default is text, but JSON is available for programmatic parsing.

```bash
APERTURE_LOG=debug APERTURE_LOG_FORMAT=json aperture api myapi users get-user --id 123
```

### `APERTURE_LOG_MAX_BODY`
Configures the maximum number of characters to include in request/response body logs. Default is 1000.

```bash
APERTURE_LOG_MAX_BODY=5000 aperture -vv api myapi users get-user --id 123
```

### `APERTURE_LOG_FILE`
Writes logs to a file instead of stderr. The file is created if it doesn't exist and logs are appended.

```bash
APERTURE_LOG=debug APERTURE_LOG_FILE=/tmp/aperture-debug.log aperture api myapi users get-user --id 123
```

This is useful for:
- Capturing logs for later analysis
- Keeping the terminal clean while debugging
- Automated testing where you need to parse logs

If the file cannot be opened (permission denied, invalid path, etc.), a warning is printed and logs fall back to stderr.

## Security Considerations

Aperture automatically redacts sensitive information in logs:

### Redacted Headers
The following headers are automatically redacted:
- `Authorization` / `Proxy-Authorization` → `[REDACTED]`
- `X-API-Key` / `X-API-Token` / `API-Key` → `[REDACTED]`
- `X-Auth-Token` / `X-Access-Token` / `X-Secret-Token` → `[REDACTED]`
- `Token` / `Secret` / `Password` → `[REDACTED]`
- `X-Webhook-Secret` → `[REDACTED]`
- `Cookie` / `Set-Cookie` → `[REDACTED]`
- `X-CSRF-Token` / `X-XSRF-Token` → `[REDACTED]`
- `X-Amz-Security-Token` → `[REDACTED]`
- `Private-Token` → `[REDACTED]`

### Redacted Query Parameters
Sensitive query parameters in URLs are automatically redacted:
- `api_key`, `apikey`, `key` → `?api_key=[REDACTED]`
- `token`, `access_token`, `auth_token` → `?token=[REDACTED]`
- `secret`, `client_secret`, `api_secret` → `?secret=[REDACTED]`
- `password`, `passwd`, `pwd` → `?password=[REDACTED]`
- `signature`, `sig` → `?signature=[REDACTED]`

### Dynamic Secret Redaction
In addition to the static header and query parameter lists above, Aperture dynamically redacts secrets configured via `x-aperture-secret` extensions in your OpenAPI spec or config-based secrets. These values are:

- **Redacted in header values**: If any header value exactly matches a configured secret
- **Redacted in request/response bodies**: If the secret appears anywhere in the body (only for secrets 8+ characters to avoid false positives)

This means your API keys and tokens configured in environment variables will never appear in logs, even if they're echoed back in error responses.

### Body Truncation
Response bodies are truncated at 1000 characters by default to avoid logging excessively large payloads. You can increase this with `APERTURE_LOG_MAX_BODY`.

## JSON Output Format

For programmatic processing, you can enable JSON output format:

```bash
APERTURE_LOG=debug APERTURE_LOG_FORMAT=json aperture api myapi users get-user --id 123
```

Example JSON output:
```json
{"level":"info","target":"aperture::executor","message":"→ GET https://api.example.com/users/123","timestamp":"2024-01-15T10:30:00Z"}
{"level":"info","target":"aperture::executor","message":"← 200 OK (143ms)","timestamp":"2024-01-15T10:30:00.143Z"}
```

## Common Troubleshooting Scenarios

### Debugging Authentication Failures
When authentication headers are missing or incorrect:

```bash
aperture -v api myapi users get-user --id 123
```

Check the output for the `Authorization` header (shown as `[REDACTED]` but the log will confirm it was sent).

### Debugging Unexpected Response Data
When the API returns unexpected data:

```bash
aperture -vv api myapi users get-user --id 123
```

The full response body will be logged, helping you identify issues with the API response.

### Debugging Network Timeouts
To see request timing information:

```bash
aperture -v api myapi users get-user --id 123
```

The `← 200 OK (XXms)` line shows how long the request took.

### Debugging Header-Related Issues
To inspect all headers being sent and received:

```bash
aperture -v api myapi users get-user --id 123
```

The debug output will show all request and response headers.

## Log Output Destination

Logs are written to `stderr` by default, leaving `stdout` clean for piping output to other commands:

```bash
# Logs go to stderr, JSON response goes to stdout
aperture -v api myapi users list | jq '.[] | select(.active)'
```

### File Output

Use `APERTURE_LOG_FILE` to write logs to a file:

```bash
# Write debug logs to a file
APERTURE_LOG=debug APERTURE_LOG_FILE=/tmp/aperture.log aperture api myapi users get-user --id 123

# View the logs
cat /tmp/aperture.log
```

File output is especially useful for:
- Long-running batch operations
- CI/CD pipelines where you need to preserve logs
- Debugging issues that require reviewing multiple requests

## Examples

### Debug a Complete API Flow
```bash
APERTURE_LOG=debug aperture api myapi posts create-post \
  --title "My Post" \
  --body "This is my post"
```

### Capture Logs for Later Analysis
```bash
# Using APERTURE_LOG_FILE (recommended)
APERTURE_LOG=debug APERTURE_LOG_FORMAT=json APERTURE_LOG_FILE=debug.log \
  aperture api myapi users get-user --id 123

# Or using shell redirection
APERTURE_LOG=debug APERTURE_LOG_FORMAT=json aperture api myapi users get-user \
  --id 123 2> debug.log
```

### Test with Verbose Output and Large Response Bodies
```bash
APERTURE_LOG_MAX_BODY=10000 aperture -vv api myapi search --query "example" | head -100
```

### Combine with Batch Operations
```bash
APERTURE_LOG=debug aperture api myapi --batch-file operations.yaml
```

## Tips and Best Practices

1. **Start with `-v`**: Use `-v` for debug output before escalating to `-vv`
2. **Use JSON format for automation**: Pipe logs to tools like `jq` for processing
3. **Keep body size reasonable**: Increase `APERTURE_LOG_MAX_BODY` only when needed
4. **Check redaction**: The `[REDACTED]` markers confirm sensitive headers are being protected
5. **Use with --dry-run**: Combine `-v` with `--dry-run` to see the request without executing it

## Related Documentation

- [Configuration Guide](configuration.md) - How to configure API specifications
- [Security Guide](security.md) - Information about authentication and secrets
- [Architecture Guide](architecture.md) - Internal design and request handling
