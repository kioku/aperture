# ADR-003: Test Isolation and Base URL Configuration

## Status
Accepted

## Context
During the implementation of HTTP request executor tests using wiremock, we encountered test failures when running tests in parallel. The root cause was that multiple tests were modifying the global `APERTURE_BASE_URL` environment variable concurrently, causing tests to send requests to the wrong mock server.

### The Problem
```rust
// Test A starts
std::env::set_var("APERTURE_BASE_URL", "http://127.0.0.1:58605");

// Test B starts (parallel execution)
std::env::set_var("APERTURE_BASE_URL", "http://127.0.0.1:58642");

// Test A continues but now uses Test B's URL
// Test A fails because its mock server doesn't receive the request
```

This is a classic concurrent modification problem with global mutable state.

## Decision
We will refactor the executor to accept the base URL as an optional parameter rather than only reading from environment variables.

```rust
pub async fn execute_request(
    spec: &CachedSpec,
    matches: &ArgMatches,
    base_url: Option<&str>  // New parameter
) -> Result<(), Error>
```

The implementation will:
1. Use the provided `base_url` if `Some`
2. Fall back to `APERTURE_BASE_URL` environment variable if `None`
3. Use a default value if the environment variable is not set

## Alternatives Considered

### 1. Force Single-Threaded Tests (`--test-threads=1`)
- **Pros**: No code changes needed
- **Cons**: Slower test execution, doesn't fix the root cause
- **Rejected**: This is a workaround, not a solution

### 2. Test-Specific Environment Variables
- **Pros**: Tests can run in parallel
- **Cons**: Hacky, pollutes environment namespace
- **Rejected**: Poor design that doesn't scale

### 3. Thread-Local Storage
- **Pros**: Thread-safe by design
- **Cons**: Complex, hard to reason about
- **Rejected**: Over-engineering for this use case

### 4. Mock at Module Level
- **Pros**: Fast, no network calls
- **Cons**: Doesn't test actual HTTP logic
- **Rejected**: Reduces test coverage quality

### 5. Serial Test Crate
- **Pros**: Minimal changes, explicit serialization
- **Cons**: Adds dependency, still slower for affected tests
- **Rejected**: Another workaround rather than fixing design

### 6. Store Base URL in CachedSpec
- **Pros**: Natural location for API configuration
- **Cons**: Requires cache model changes, couples spec to deployment
- **Rejected**: Base URL is deployment-specific, not spec-specific

## Consequences

### Positive
- **Test Isolation**: Tests are completely independent
- **Parallel Execution**: All tests can run concurrently
- **Better Design**: Explicit dependencies instead of hidden global state
- **Flexibility**: Different base URLs can be used in the same process
- **Testability**: Easier to test with different configurations

### Negative
- **API Change**: Function signature must be updated
- **Call Site Updates**: All callers must be modified

### Neutral
- **Environment Variable Still Supported**: Production behavior unchanged
- **Backward Compatible**: Passing `None` maintains current behavior

## Implementation Notes

1. Update `execute_request` signature
2. Modify the function to check parameter first, then environment
3. Update `main.rs` to pass `None` (maintaining current behavior)
4. Update tests to pass mock server URLs directly
5. Remove `std::env::set_var` calls from tests

This design follows the Dependency Injection pattern, making dependencies explicit and improving testability without sacrificing convenience for production use.