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
        }
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

The `metadata` field is optional and used for documentation purposes. Only `operations` is required.

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
    FAILED=$(echo "$RESULT" | jq '[.results[] | select(.status == "error")]')
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
| Startup time | ~8ms | Low latency per invocation |
| Binary size | 3.7MB | Fast container deployment |
| Memory (typical) | 3-5 MB | Low resource footprint |
| Spec loading | O(1) | Pre-parsed binary cache |

For high-frequency usage, the binary cache strategy ensures consistent latency regardless of spec complexity—the OpenAPI spec is parsed once during `config add`, not on every invocation.
