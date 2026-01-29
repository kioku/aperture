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

## Security Considerations

Aperture automatically redacts sensitive information in logs:

### Redacted Headers
The following headers are automatically redacted:
- `Authorization` → `Bearer [REDACTED]`
- `X-API-Key` → `[REDACTED]`
- `X-Auth-Token` → `[REDACTED]`
- `X-Access-Token` → `[REDACTED]`
- `API-Key` → `[REDACTED]`
- `Token` → `[REDACTED]`
- `X-Secret-Token` → `[REDACTED]`
- `Password` → `[REDACTED]`

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

## Examples

### Debug a Complete API Flow
```bash
APERTURE_LOG=debug aperture api myapi posts create-post \
  --title "My Post" \
  --body "This is my post"
```

### Capture Logs for Later Analysis
```bash
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
