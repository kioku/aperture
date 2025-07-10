# Phase 3 Features: Automation at Scale & Experimental Syntax

This document provides detailed documentation for the Phase 3 features implemented in Aperture CLI v0.1.2+. These features focus on automation at scale and introduce experimental syntax for future releases.

## Overview

Phase 3 introduces three major feature categories:

1. **Bulk Operations & Request Caching** - For high-volume automation
2. **Experimental Flag-Based Parameter Syntax** - New command syntax for enhanced usability
3. **Cache Management** - Tools for managing response caches

## Feature 3.1: Bulk Operations & Request Caching

### Batch Processing

Aperture now supports executing multiple API operations from a single batch file, with concurrency control and rate limiting.

#### Basic Usage

```bash
# Execute operations from a batch file
aperture --batch-file operations.json api my-api

# Control concurrency (default: 5)
aperture --batch-file operations.json --batch-concurrency 10 api my-api

# Rate limiting (requests per second)
aperture --batch-file operations.json --batch-rate-limit 50 api my-api
```

#### Batch File Format

Batch files can be written in JSON or YAML format:

**JSON Format:**
```json
{
  "metadata": {
    "name": "User Management Batch",
    "description": "Create and retrieve multiple users",
    "version": "1.0"
  },
  "operations": [
    {
      "id": "create-user-1",
      "args": ["users", "create-user", "--body", "{\"name\": \"Alice\", \"email\": \"alice@example.com\"}"],
      "description": "Create user Alice"
    },
    {
      "id": "get-user-1",
      "args": ["users", "get-user-by-id", "--id", "123"],
      "description": "Retrieve user by ID"
    }
  ]
}
```

**YAML Format:**
```yaml
metadata:
  name: User Management Batch
  description: Create and retrieve multiple users
  version: "1.0"
operations:
  - id: create-user-1
    args: [users, create-user, --body, '{"name": "Alice", "email": "alice@example.com"}']
    description: Create user Alice
  - id: get-user-1
    args: [users, get-user-by-id, --id, "123"]
    description: Retrieve user by ID
```

#### Advanced Batch Configuration

Each operation can include custom headers and cache settings:

```json
{
  "operations": [
    {
      "id": "authenticated-request",
      "args": ["users", "get-user-by-id", "--id", "123"],
      "headers": {
        "X-Request-ID": "unique-request-id",
        "X-Custom-Header": "custom-value"
      },
      "use_cache": true,
      "description": "Request with custom headers and caching"
    }
  ]
}
```

### Response Caching

Aperture includes intelligent response caching to improve performance for repeated requests.

#### Basic Caching

```bash
# Enable caching with default TTL (300 seconds)
aperture api my-api --cache users list

# Custom TTL in seconds
aperture api my-api --cache --cache-ttl 600 users list

# Explicitly disable caching
aperture api my-api --no-cache users list
```

#### Cache Management

```bash
# View cache statistics
aperture config cache-stats my-api

# Clear cache for specific API
aperture config clear-cache my-api

# Clear all caches
aperture config clear-cache --all
```

#### Cache Key Generation

Cache keys are generated based on:
- API specification name
- HTTP method and path
- Query parameters
- Request body content
- Authentication headers (excluded for security)

#### Cache Storage

- **Location:** `~/.config/aperture/.cache/responses/`
- **Format:** JSON files with metadata
- **TTL:** Configurable per request (default: 300 seconds)
- **Security:** Authentication headers are excluded from cache keys

## Feature 3.2: Experimental Flag-Based Parameter Syntax

### Overview

The experimental flag-based parameter syntax provides a more consistent and predictable command-line interface by converting all parameters (including path parameters) to flags.

#### Comparison

**Traditional Syntax:**
```bash
# Path parameters are positional
aperture api my-api users get-user-by-id 123 --include-profile true
```

**Experimental Syntax:**
```bash
# All parameters are flags
aperture api my-api --experimental-flags users get-user-by-id --id 123 --include-profile true
```

#### Benefits

1. **Consistency:** All parameters use the same flag-based syntax
2. **Clarity:** Parameter names are explicit in the command
3. **Flexibility:** Parameters can be provided in any order
4. **Scripting:** Easier to generate commands programmatically

#### Usage

```bash
# Enable experimental syntax globally
aperture --experimental-flags api my-api users get-user-by-id --id 123

# Works with all parameter types
aperture --experimental-flags api my-api users create-user \
  --body '{"name": "John", "email": "john@example.com"}' \
  --x-request-id "unique-id"
```

#### Backwards Compatibility

The experimental syntax is fully backwards compatible. Traditional positional syntax remains the default and will continue to work.

## Implementation Details

### Architecture

#### Batch Processing (`src/batch.rs`)

- **Concurrency Control:** Uses `tokio::sync::Semaphore` for limiting concurrent requests
- **Rate Limiting:** Implements `governor` crate for request throttling
- **Error Handling:** Configurable continue-on-error behavior
- **Progress Reporting:** Optional progress display during batch execution

#### Response Caching (`src/response_cache.rs`)

- **Cache Keys:** SHA256 hashes of normalized request parameters
- **TTL Management:** Automatic expiration based on cached timestamp
- **Storage:** File-based cache with JSON serialization
- **Cleanup:** Automatic removal of expired entries

#### Experimental Syntax (`src/engine/generator.rs`)

- **Command Generation:** Modified `generate_command_tree_with_flags` function
- **Parameter Mapping:** Converts all parameters to long flags
- **Help Generation:** Updated help text to reflect flag-based syntax

### Testing

Phase 3 features include comprehensive test coverage:

- **26 Integration Tests:** Cover all Phase 3 functionality
- **Batch Processing:** File parsing, execution, error handling
- **Response Caching:** TTL, expiration, key generation
- **Experimental Flags:** Syntax validation, backwards compatibility
- **End-to-End:** Full feature integration testing

### Performance Considerations

#### Batch Operations

- **Concurrency:** Default limit of 5 concurrent requests (configurable)
- **Rate Limiting:** Optional requests-per-second throttling
- **Memory Usage:** Streaming processing for large batch files
- **Error Recovery:** Configurable failure handling strategies

#### Response Caching

- **Cache Hits:** Sub-millisecond response times for cached requests
- **Storage Efficiency:** Compressed JSON with minimal metadata
- **TTL Management:** Efficient expiration without background processes
- **Security:** Authentication headers excluded from cache keys

## Migration Guide

### From Traditional to Experimental Syntax

1. **Identify Path Parameters:** Find parameters that are currently positional
2. **Add Flag Names:** Convert positional arguments to `--parameter-name value`
3. **Enable Experimental Mode:** Add `--experimental-flags` to your commands
4. **Test Thoroughly:** Verify all commands work with the new syntax

### Batch File Creation

1. **Start Simple:** Begin with basic operations in JSON format
2. **Add Metadata:** Include descriptive information for better organization
3. **Test Incrementally:** Start with small batches and increase size gradually
4. **Monitor Performance:** Use concurrency and rate limiting as needed

## Best Practices

### Batch Processing

- **Size Limits:** Keep batch files under 1000 operations for optimal performance
- **Error Handling:** Use `continue_on_error: true` for non-critical operations
- **Progress Monitoring:** Enable progress reporting for long-running batches
- **Resource Management:** Use appropriate concurrency limits for your API

### Response Caching

- **TTL Selection:** Choose appropriate cache lifetimes based on data freshness requirements
- **Cache Hygiene:** Regularly clear caches for frequently updated data
- **Security:** Never cache responses containing sensitive information
- **Performance:** Monitor cache hit rates and adjust TTL accordingly

### Experimental Features

- **Testing:** Thoroughly test experimental syntax in non-production environments
- **Documentation:** Document any scripts using experimental features
- **Feedback:** Report issues and suggestions for experimental features
- **Migration Planning:** Prepare for eventual migration to stable syntax

## Future Roadmap

Phase 3 features lay the groundwork for future enhancements:

- **Stable Flag Syntax:** Experimental syntax will become the default in v0.2.0
- **Advanced Batching:** Support for conditional operations and dependencies
- **Distributed Caching:** Shared cache across multiple Aperture instances
- **Plugin System:** Extensible architecture for custom batch processors

## Troubleshooting

### Common Issues

#### Batch Processing

**Issue:** Batch operations failing with authentication errors
**Solution:** Ensure environment variables are set correctly and consider using per-operation headers

**Issue:** High memory usage during batch processing
**Solution:** Reduce `--batch-concurrency` or process smaller batch files

#### Response Caching

**Issue:** Cached responses are stale
**Solution:** Reduce `--cache-ttl` or use `--no-cache` for dynamic data

**Issue:** Cache directory growing large
**Solution:** Regularly run `aperture config clear-cache --all`

#### Experimental Syntax

**Issue:** Commands fail with experimental flags
**Solution:** Verify all path parameters are converted to flags with `--parameter-name`

**Issue:** Help text is confusing
**Solution:** Use `--help` with experimental flags to see flag-based parameter documentation

### Performance Optimization

1. **Batch Concurrency:** Adjust based on API rate limits and server capacity
2. **Cache TTL:** Balance between performance and data freshness
3. **Rate Limiting:** Use to prevent API quota exhaustion
4. **Progress Reporting:** Disable for automated scripts to reduce overhead

## Contributing

To contribute to Phase 3 features:

1. **Test Coverage:** Add tests for new functionality
2. **Documentation:** Update this document with new features
3. **Backwards Compatibility:** Ensure existing functionality continues to work
4. **Performance:** Consider impact on batch processing and caching performance

For implementation details, see:
- `src/batch.rs` - Batch processing implementation
- `src/response_cache.rs` - Response caching system
- `src/engine/generator.rs` - Experimental syntax generation
- `tests/phase3_integration_tests.rs` - Comprehensive test suite