# ADR-008: Cache Policy for Authenticated Requests

## Status

Accepted

## Context

The Aperture architecture documentation (§6) states:

> "Aperture enforces a strict separation of configuration (the OpenAPI spec) and secrets."

However, the response caching system (`src/response_cache.rs`) was storing request headers—including `Authorization`, `X-API-Key`, and other authentication headers—in plaintext JSON cache files via `CachedRequestInfo`. This violated the documented security model and exposed secrets on disk.

**Evidence of the vulnerability:**

- `src/engine/executor.rs` → `store_in_cache()` wrote all headers to `CachedRequestInfo`
- `src/response_cache.rs` → `CachedResponse.request_info.headers` persisted to JSON files
- Cache files stored in `~/.config/aperture/.cache/responses/` were readable plaintext

**Security Impact:**

- Credentials persisted to disk in plaintext
- Cache files could be accessed by other processes or users
- Backup systems could capture and retain secrets
- Violated principle of least privilege for secret storage

## Decision

We implement a two-layer defense strategy:

### 1. Header Scrubbing (Defense in Depth)

All authentication headers are scrubbed from `CachedRequestInfo` before writing to disk:

```rust
pub fn scrub_auth_headers(headers: &HashMap<String, String>) -> HashMap<String, String> {
    headers
        .iter()
        .filter(|(key, _)| !is_auth_header(key))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}
```

Headers filtered include:
- `Authorization`
- `Proxy-Authorization`
- `X-API-Key`, `X-API-Token`
- `X-Auth-*` (prefix match)
- `Api-Key`, `Token`, `Bearer`, `Cookie`

### 2. Authenticated Request Caching Disabled by Default

Requests containing authentication headers are not cached by default:

```rust
pub struct CacheConfig {
    // ...
    pub allow_authenticated: bool,  // Default: false
}
```

In `prepare_cache_context()`:

```rust
let has_auth_headers = headers.iter().any(|(k, _)| is_auth_header(k.as_str()));
if has_auth_headers && !cache_cfg.allow_authenticated {
    return Ok(None);  // Skip caching
}
```

### Opt-In Mechanism

Users who need caching for authenticated requests can opt in by setting `allow_authenticated: true` in their cache configuration. Even with this enabled, auth headers are still scrubbed—the opt-in only controls whether the response is cached, not whether secrets are stored.

## Consequences

### Positive

- **Security**: Auth headers never written to disk, regardless of configuration
- **Secure Default**: Authenticated requests not cached unless explicitly enabled
- **Defense in Depth**: Two independent protections (scrubbing + skip-by-default)
- **Backward Compatible**: Existing unauthenticated caching behavior unchanged
- **Minimal Performance Impact**: Header filtering is O(n) on header count

### Negative

- **Reduced Cache Hit Rate**: Authenticated requests won't benefit from caching by default
- **Configuration Required**: Users must opt-in if they want authenticated caching
- **No TOML Support Yet**: `allow_authenticated` currently only configurable programmatically

### Neutral

- Cache key generation already excluded auth headers (for correct cache matching)
- No migration needed—existing cache files with auth headers will naturally expire

## Future Considerations

1. **TOML Configuration**: Add `[cache]` section to `config.toml`:
   ```toml
   [cache]
   enabled = true
   allow_authenticated = false
   default_ttl_secs = 300
   ```

2. **Per-Operation Cache Control**: Support `x-aperture-cache` extension in OpenAPI specs:
   ```yaml
   x-aperture-cache:
     enabled: true
     allow_authenticated: true
     ttl: 60
   ```

3. **Cache Encryption**: For highly sensitive environments, encrypt cache files at rest

4. **Audit Logging**: Log when authenticated responses are cached (if opted in)

## References

- Issue: #67 (arch: Response cache violates secret boundary by storing auth headers)
- Architecture: `docs/architecture.md` §6 (Secret Management)
- Implementation: `src/response_cache.rs`, `src/engine/executor.rs`
