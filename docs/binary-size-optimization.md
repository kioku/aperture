# Binary Size Optimization

## Overview

This document describes the binary size optimization strategies implemented for Aperture CLI, achieving a **67% reduction** in release binary size from 11MB to 3.6MB.

## Optimization Results

| Configuration | Binary Size | Reduction |
|---------------|------------|-----------|
| Original (unoptimized) | 11MB | - |
| Compiler optimizations | 4.0MB | 64% |
| + Dependency optimization | 3.6MB | 67% |
| Size-optimized profile | 5.0MB | 55% |

## Applied Optimizations

### 1. Compiler Optimizations (Release Profile)

```toml
[profile.release]
strip = true          # Strip symbols
opt-level = "z"       # Optimize for size over speed
lto = "fat"          # Link-time optimization
codegen-units = 1    # Single codegen unit
panic = "abort"      # Smaller panic handler
```

### 2. Dependency Optimization

- Removed unnecessary HTTP/2 support from `reqwest`
- Changed from default features to minimal feature set:
  ```toml
  reqwest = { version = "0.12.21", default-features = false, features = ["json", "native-tls"] }
  ```

### 3. Build Profiles

Three profiles are available for different use cases:

#### Release Profile (Default)
- **Size**: 3.6MB
- **Use**: General production deployments
- **Command**: `cargo build --release`

#### Minimal Profile
- **Size**: 3.6MB (same as release, already maximally optimized)
- **Use**: When smallest possible binary is critical
- **Command**: `cargo build --profile minimal`

#### Size-Optimized Profile
- **Size**: 5.0MB
- **Use**: Better balance between size and performance
- **Command**: `cargo build --profile size-optimized`
- **Differences**: Uses `opt-level = "s"` and `lto = "thin"`

## Trade-offs

### Performance Impact
- **opt-level = "z"**: ~10-20% slower execution compared to default
- **lto = "fat"**: Longer compile times (+30-60s)
- **codegen-units = 1**: Slower parallel compilation

### Functionality Impact
- **panic = "abort"**: No stack unwinding on panic (faster panics, smaller binary)
- **No HTTP/2**: HTTP/1.1 only (sufficient for most REST APIs)

## Measurement Tools

Monitor binary size during development:

```bash
# Check binary size
ls -lh target/release/aperture

# Analyze crate contributions (requires cargo-bloat)
cargo install cargo-bloat
cargo bloat --release --crates

# Detailed symbol analysis
nm -S target/release/aperture | sort -k2 -nr | head -20
```

## Future Optimization Opportunities

1. **Further dependency auditing**: Review feature flags for all dependencies
2. **Code-level optimizations**: 
   - Reduce string allocations
   - Consolidate error variants
   - Use `&'static str` where possible
3. **Alternative allocators**: Consider using `jemalloc` or `mimalloc`
4. **Build with nightly**: Additional optimizations with unstable features

## Recommendations

- Use the default release profile for production deployments (3.6MB)
- Consider size-optimized profile if performance regression is noticeable
- Monitor binary size in CI/CD to prevent regression
- Document any new dependencies and their feature requirements