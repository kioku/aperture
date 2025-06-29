# ADR-001: Dynamic Command Generation String Lifetimes

## Status
Accepted

## Context
When implementing the dynamic command generator for Aperture (Phase 4), we encountered a significant challenge with clap 4.5.40's string lifetime requirements. The clap library requires `'static` lifetimes for command names, argument names, and other string values when building command trees dynamically.

Our `CachedSpec` structure contains owned `String` fields that are loaded at runtime from cached binary files. We needed to convert these runtime strings into the `'static` lifetime references that clap expects.

### The Problem
```rust
// This doesn't compile - clap expects &'static str, not &String
let command = Command::new(&spec.name);

// This also doesn't compile - temporary value dropped
let command = Command::new(spec.name.as_str());
```

## Decision
We chose to use `Box::leak` to convert `String` values into `&'static str` references. This approach intentionally leaks memory by allocating strings on the heap and never deallocating them, effectively giving them a `'static` lifetime.

```rust
fn to_static_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
```

## Consequences

### Positive
- **Simplicity**: The solution is straightforward and easy to understand
- **Performance**: No runtime overhead after initial allocation
- **Compatibility**: Works perfectly with clap's API without requiring unsafe code
- **Reliability**: No risk of use-after-free or dangling references

### Negative
- **Memory Usage**: Leaked memory accumulates for each API command generation
- **Not Suitable for Long-Running Processes**: In a server context, this would be problematic
- **One-Way Operation**: Cannot reclaim the leaked memory

### Neutral
- **Acceptable for CLI Tool**: Since Aperture is a short-lived CLI process, the memory leak is inconsequential - the OS reclaims all memory when the process exits

## Alternatives Considered

1. **Static Command Trees**: Pre-define all possible commands at compile time
   - Rejected: Defeats the purpose of dynamic generation from OpenAPI specs

2. **Arc<str> or Rc<str>**: Use reference-counted strings
   - Rejected: Clap doesn't accept these types, still requires `'static`

3. **Unsafe Lifetime Extension**: Use unsafe code to extend lifetimes
   - Rejected: Introduces potential for undefined behavior

4. **Fork/Patch Clap**: Modify clap to accept owned strings
   - Rejected: Maintenance burden and compatibility issues

5. **Alternative CLI Libraries**: Use a library that supports owned strings
   - Rejected: Clap is the de facto standard with best ecosystem support

## Notes
This approach is specifically suitable for CLI applications where:
- The process is short-lived
- The amount of leaked memory is bounded and small (API spec metadata)
- Simplicity and correctness are prioritized over memory efficiency

For future versions, if Aperture evolves into a long-running service, this decision should be revisited.