# ADR-002: HTTP Request Executor Design

## Status
Accepted

## Context
When implementing Phase 4 of Aperture (Dynamic Command Generation & Execution Engine), we needed to design the HTTP request executor that would:
- Map CLI arguments back to API operations
- Build and execute HTTP requests
- Handle authentication and headers
- Format responses for both human and agent consumption

The executor needed to work seamlessly with the dynamic command generator while maintaining flexibility for future enhancements.

## Decision
We implemented a multi-stage executor with clear separation of concerns:

1. **Operation Mapping**: Find the correct `CachedCommand` from the CLI argument hierarchy
2. **URL Building**: Construct URLs with path parameter substitution and query parameters
3. **Header Management**: Build headers including authentication (placeholder for x-aperture-secret)
4. **Request Execution**: Use reqwest for async HTTP calls
5. **Response Handling**: Pretty-print JSON responses and provide clear error messages

### Key Implementation Choices

#### 1. Placeholder Authentication
We implemented a TODO placeholder for x-aperture-secret authentication rather than full implementation because:
- The cached spec doesn't currently include security scheme information
- This would require enhancing the cache model to store security mappings
- It's better to implement this properly in a future phase

#### 2. Base URL from Environment
We chose to read the base URL from `APERTURE_BASE_URL` environment variable:
- Follows 12-factor app principles
- Allows easy switching between environments
- Aligns with how secrets will be handled

#### 3. Response Formatting
We automatically pretty-print JSON responses when possible:
- Improves human readability
- Falls back to raw text for non-JSON responses
- Agents can still parse the output

#### 4. Error Messages with Context
Error messages include the full response body for failed requests:
- Helps with debugging API issues
- Provides context for both humans and agents
- Preserves the original error information

## Consequences

### Positive
- **Clean Architecture**: Each function has a single responsibility
- **Testability**: Components can be tested independently
- **Extensibility**: Easy to add authentication, retry logic, etc.
- **User-Friendly**: Clear error messages and pretty-printed output

### Negative
- **Incomplete Authentication**: x-aperture-secret support is not implemented
- **Limited Error Handling**: No retry logic or timeout configuration
- **Basic Response Validation**: No schema validation against OpenAPI spec

### Neutral
- **Synchronous Feel**: Despite being async, the executor blocks until completion
- **Console Output**: Responses go directly to stdout (appropriate for CLI)

## Future Enhancements
1. Implement x-aperture-secret authentication mapping
2. Add response schema validation
3. Support configurable timeouts and retries
4. Add `--raw` flag to disable pretty-printing
5. Implement progress indicators for long-running requests