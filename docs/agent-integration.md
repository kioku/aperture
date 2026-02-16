# Agent Integration Guide

Aperture is designed as an API execution runtime for autonomous AI agents. This guide covers the features that make Aperture agent-friendly and patterns for integrating it into agentic workflows.

## Design Philosophy

Traditional API CLI tools optimize for human developers: interactive prompts, colorized output, verbose help text. Aperture takes a different approach:

- **Structured I/O**: JSON in, JSON out—no parsing HTML error pages
- **Predictable Errors**: Machine-readable error codes with actionable context
- **Fast Invocation**: Sub-10ms startup for high-frequency tool calling
- **Self-Describing**: Capability manifests for runtime API discovery
- **Stateless Execution**: Each invocation is independent and idempotent-safe

## Capability Manifest

The `--describe-json` flag outputs a complete manifest of available API operations, enabling agents to discover capabilities at runtime without parsing help text.

```bash
aperture api my-api --describe-json
```

**Output structure:**

```json
{
  "api": {
    "name": "My API",
    "version": "1.0.0",
    "description": "API description",
    "base_url": "https://api.example.com"
  },
  "commands": {
    "users": [
      {
        "name": "get-user-by-id",
        "method": "GET",
        "path": "/users/{id}",
        "description": "Retrieve a user by ID",
        "summary": "Get user by ID",
        "operation_id": "getUserById",
        "parameters": [
          {
            "name": "id",
            "location": "path",
            "required": true,
            "param_type": "string",
            "description": "User ID"
          }
        ],
        "request_body": null,
        "security_requirements": ["bearerAuth"],
        "tags": ["users"],
        "response_schema": {
          "content_type": "application/json",
          "schema": {"type": "object", "properties": {"id": {"type": "integer"}}},
          "example": {"id": 123, "name": "Alice"}
        },
        "display_name": "fetch",
        "display_group": "accounts",
        "aliases": ["get", "show"],
        "hidden": false
      }
    ]
  },
  "security_schemes": {
    "bearerAuth": {
      "type": "http",
      "scheme": "bearer"
    }
  }
}
```

**Command mapping fields in the manifest:**

| Field | Type | Description |
|-------|------|-------------|
| `display_name` | `string?` | Effective subcommand name (from command mapping). Omitted if no rename. |
| `display_group` | `string?` | Effective group name (from command mapping). Omitted if no rename. |
| `aliases` | `string[]` | Additional subcommand aliases. Omitted if empty. |
| `hidden` | `boolean` | Whether the command is hidden from help output. Omitted if `false`. |

When command mappings are configured, the manifest groups commands by their **effective** group name (i.e., `display_group` if set, otherwise the original tag). Agents should use the manifest's group keys and `name`/`display_name` fields to construct correct CLI invocations.

*Note: Additional metadata fields such as `deprecated`, `external_docs_url`, `original_tags` on commands, and `description`, `x-aperture-secret` on security schemes may also be present.*

### Response Schema Limitations

The `response_schema` field provides schema information for successful responses (200/201/204), but has limitations:

- **Schema `$ref` references are resolved**: Top-level references like `$ref: '#/components/schemas/User'` are expanded inline.
- **Response references are NOT resolved**: If a response is defined as `$ref: '#/components/responses/UserResponse'`, the schema will not be extracted.
- **Nested references remain as-is**: References within object properties are not recursively resolved.

**Usage patterns:**

```bash
# List all available operations
aperture api my-api --describe-json | jq '.commands | keys'

# Get parameters for a specific operation
aperture api my-api --describe-json | jq '.commands.users[] | select(.name == "get-user-by-id")'

# Check authentication requirements
aperture api my-api --describe-json | jq '.security_schemes'
```

## Structured Errors

The `--json-errors` flag ensures all errors are output as structured JSON, enabling programmatic error handling.

```bash
aperture api my-api --json-errors users get-user-by-id --id 123
```

**Error response structure:**

```json
{
  "error_type": "Authentication",
  "message": "Environment variable 'API_TOKEN' is not set",
  "context": "Set the environment variable before retrying",
  "details": {
    "scheme_name": "bearerAuth",
    "env_var": "API_TOKEN"
  }
}
```

**Error categories:**

| Type | Description |
|------|-------------|
| `Specification` | Spec not found, corrupted cache |
| `Authentication` | Missing secrets, invalid credentials |
| `Validation` | Invalid input parameters |
| `Network` | Connection failures, DNS errors, timeouts |
| `HttpError` | HTTP 4xx/5xx responses |
| `Headers` | Invalid header names or values |
| `ServerVariable` | Template variable resolution errors |
| `Runtime` | General operational errors |

## Quiet Mode

The `-q` or `--quiet` flag suppresses informational messages, outputting only data. Essential for clean JSON pipelines in agent workflows.

```bash
# Without quiet mode
aperture api my-api users list
# Output includes: "Fetching users..." and other status messages

# With quiet mode - only JSON data
aperture api my-api --quiet users list
# Output: {"users": [...]}

# Combine with jq for clean pipelines
aperture api my-api -q users list | jq '.users[].id'
```

**Behavior by command:**
- API operations: Suppresses progress/status messages, outputs only response data
- Config commands: Suppresses confirmation messages, outputs only requested data
- Batch operations: Suppresses per-operation progress, outputs only final summary

## Dry Run Mode

The `--dry-run` flag shows the HTTP request that would be made without executing it. Useful for validation and debugging.

```bash
aperture api my-api --dry-run users create --name "Test User"
```

**Output:**

```
POST https://api.example.com/users
Authorization: Bearer <redacted>
Content-Type: application/json

{"name": "Test User"}
```

## Batch Operations

For high-volume automation, batch processing executes multiple operations with concurrency control and rate limiting.

**Batch file format (JSON):**

```json
{
  "metadata": {
    "name": "User data collection",
    "description": "Fetch multiple users"
  },
  "operations": [
    {
      "id": "user-1",
      "args": ["users", "get-user-by-id", "--id", "123"]
    },
    {
      "id": "user-2",
      "args": ["users", "get-user-by-id", "--id", "456"]
    },
    {
      "id": "user-3",
      "args": ["users", "get-user-by-id", "--id", "789"]
    }
  ]
}
```

**Field requirements:**
- `operations` is the only required field
- `metadata` is optional, used for documentation
- `id` within each operation is optional but recommended for tracking results

**Execution:**

```bash
# Execute with concurrency limit
aperture api my-api --batch-file operations.json --batch-concurrency 5

# With rate limiting (requests per second)
aperture api my-api --batch-file operations.json --batch-rate-limit 10

# With JSON error output for parsing results
aperture api my-api --batch-file operations.json --json-errors
```

**Batch result structure:**

```json
{
  "batch_execution_summary": {
    "total_operations": 3,
    "successful_operations": 2,
    "failed_operations": 1,
    "total_duration_seconds": 0.45,
    "operations": [
      {"operation_id": "user-1", "args": ["users", "get-user-by-id", "--id", "123"], "success": true, "duration_seconds": 0.12, "error": null},
      {"operation_id": "user-2", "args": ["users", "get-user-by-id", "--id", "456"], "success": true, "duration_seconds": 0.15, "error": null},
      {"operation_id": "user-3", "args": ["users", "get-user-by-id", "--id", "789"], "success": false, "duration_seconds": 0.18, "error": "HTTP 404: User not found"}
    ]
  }
}
```

## Automatic Retry with Exponential Backoff

Aperture supports automatic retries for transient failures, with exponential backoff and jitter. This is essential for reliable agent workflows interacting with rate-limited or occasionally unavailable APIs.

### CLI Flags

```bash
# Retry up to 3 times with default delays
aperture api my-api --retry 3 users list

# Custom initial delay (default: 500ms)
aperture api my-api --retry 3 --retry-delay 1s users list

# Custom maximum delay cap (default: 30s)
aperture api my-api --retry 3 --retry-max-delay 60s users list

# Force retry on non-idempotent requests (use with caution)
aperture api my-api --retry 3 --force-retry users create --name "Test"
```

### Persistent Configuration

Configure default retry behavior in `config.toml`:

```bash
# Enable retries globally (3 attempts)
aperture config set retry_defaults.max_attempts 3

# Set initial delay to 1 second
aperture config set retry_defaults.initial_delay_ms 1000

# Set maximum delay to 60 seconds
aperture config set retry_defaults.max_delay_ms 60000
```

CLI flags override configured defaults.

### Retry Behavior

**Retryable conditions:**
- HTTP 429 (Too Many Requests)
- HTTP 503 (Service Unavailable)
- HTTP 5xx (Server Errors)
- Network timeouts and connection errors

**Exponential backoff:**
- Delay doubles after each attempt: 500ms → 1s → 2s → 4s...
- Jitter added to prevent thundering herd
- Capped at `max_delay` (default 30s)

**Retry-After header:**
- If the server returns a `Retry-After` header, Aperture respects it
- The header value overrides calculated backoff for that attempt

### Safety for Non-Idempotent Requests

By default, Aperture only retries **idempotent** HTTP methods (GET, HEAD, OPTIONS, PUT, DELETE). For non-idempotent methods (POST, PATCH):

```bash
# Safe: Use idempotency key for POST requests
aperture api my-api --retry 3 --idempotency-key "order-123" orders create --item "widget"

# Override safety check (use only when you understand the risks)
aperture api my-api --retry 3 --force-retry orders create --item "widget"
```

### Retry Information in JSON Errors

With `--json-errors`, failed requests include retry details:

```json
{
  "error_type": "HttpError",
  "message": "HTTP 503: Service Unavailable",
  "retry_info": {
    "attempts": 3,
    "total_delay_ms": 3500,
    "final_status": 503,
    "retryable": true
  }
}
```

### Batch Operations with Retries

Retries apply per-operation in batch mode:

```bash
aperture api my-api --batch-file ops.json --retry 3 --json-errors
```

Each operation in the batch is retried independently. The batch summary includes retry statistics:

```json
{
  "batch_execution_summary": {
    "operations": [
      {
        "operation_id": "op-1",
        "success": true,
        "retry_info": {"attempts": 2, "total_delay_ms": 500}
      }
    ]
  }
}
```

## Idempotency Keys

For safe retries, pass an idempotency key to ensure duplicate requests are handled correctly by the API:

```bash
aperture api my-api --idempotency-key "txn-12345" payments create --amount 100
```

The key is sent as the `Idempotency-Key` header.

## Exit Codes

Aperture uses standard exit codes for automation:

| Code | Meaning |
|------|---------|
| `0` | Success—all operations completed |
| `1` | Failure—one or more operations failed |

For batch operations, exit code `1` indicates ANY operation failed:

```bash
aperture api my-api --batch-file ops.json --json-errors
if [ $? -eq 0 ]; then
    echo "All operations succeeded"
else
    echo "Some operations failed—check JSON output"
fi
```

## Response Filtering

Extract specific fields from responses using JQ syntax:

```bash
# Basic field access (always available)
aperture api my-api users get-user-by-id --id 123 --jq '.name'
aperture api my-api users list --jq '.users[0].email'

# Advanced filtering (requires --features jq)
aperture api my-api users list --jq '[.users[] | select(.active == true)]'
```

Filter the capability manifest:

```bash
# List operation names
aperture api my-api --describe-json --jq '.commands | to_entries | .[].value[].name'

# Find POST operations
aperture api my-api --describe-json --jq '[.commands[][] | select(.method == "POST")]'
```

## Integration Patterns

### Pattern 1: Discovery → Execute

```bash
# Agent discovers available operations
MANIFEST=$(aperture api my-api --describe-json)

# Agent selects operation based on task
OPERATION=$(echo "$MANIFEST" | jq -r '.commands.users[] | select(.name == "get-user-by-id")')

# Agent executes with parameters
aperture api my-api --json-errors users get-user-by-id --id 123
```

### Pattern 2: Batch with Error Recovery

```bash
# Execute batch
RESULT=$(aperture api my-api --batch-file ops.json --json-errors 2>&1)
EXIT_CODE=$?

if [ $EXIT_CODE -ne 0 ]; then
    # Extract failed operations for retry
    FAILED=$(echo "$RESULT" | jq '[.batch_execution_summary.operations[] | select(.success == false)]')
    # Handle failures...
fi
```

### Pattern 3: Cached Responses

For read-heavy workflows, enable response caching:

```bash
# Cache responses for 5 minutes
aperture api my-api --cache --cache-ttl 300 users list

# Subsequent calls within TTL return cached data
aperture api my-api --cache users list  # Fast, from cache
```

## Performance Considerations

Aperture is optimized for agent invocation patterns:

| Metric | Value | Impact |
|--------|-------|--------|
| Startup time | < 10ms | Low latency per invocation |
| Binary size | ~4.0MB | Fast container deployment |
| Memory (typical) | 3-5 MB | Low resource footprint |
| Spec loading | O(1) | Pre-parsed binary cache |

For high-frequency usage, the binary cache strategy ensures consistent latency regardless of spec complexity—the OpenAPI spec is parsed once during `config add`, not on every invocation.
