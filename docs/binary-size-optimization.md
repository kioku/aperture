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

## Code-Level Optimization Analysis

### Investigation Findings

After detailed analysis using cargo-bloat and code inspection, we identified several opportunities for further size reduction:

| Optimization Area | Current Impact | Potential Savings | Risk Level |
|------------------|----------------|-------------------|------------|
| Error Handling | 22KB (to_json alone) | 300-500KB | Medium |
| String Allocations | 1,513 occurrences | 100-200KB | Low |
| Generic Monomorphization | 160 generic functions | 100-300KB | High |
| Async Function Size | 69KB (run_command) | 50-150KB | Medium |
| Dependency Features | Multiple unused | 50-100KB | Low |

**Total potential reduction**: 600KB-1.25MB (could reach 2.6-3.0MB final size)

## Detailed Code-Level Optimization Plan

### 1. Error Handling Consolidation (High Priority)

**Problem Analysis:**
- 50+ error variants generating unique code paths
- `Error::to_json()` method alone uses 22KB
- 97 string allocations in error.rs
- Extensive match arms for each variant (360+ lines)

**Solution Approach:**
```rust
// Before: 50+ specific variants
enum Error {
    SpecNotFound { name: String },
    SpecAlreadyExists { name: String },
    CachedSpecNotFound { name: String },
    // ... 47 more variants
}

// After: Consolidated approach
enum ErrorKind {
    NotFound,
    AlreadyExists,
    Validation,
    Network,
    // ... ~10-15 core variants
}

struct Error {
    kind: ErrorKind,
    context: ErrorContext,
}
```

**Implementation Checklist:**
- [ ] Create ErrorKind enum with core variants
- [ ] Implement ErrorContext for details
- [ ] Replace string literals with `&'static str` constants
- [ ] Write macro for repetitive JSON conversion
- [ ] Update all error creation sites
- [ ] Validate error message quality

### 2. String Allocation Optimization (Medium Priority)

**Problem Analysis:**
- 1,513 total string allocations found
- Common antipattern: `"literal".to_string()`
- Unnecessary `format!` for simple concatenation
- String cloning where references suffice

**Solution Approach:**
```rust
// Before
fn get_message() -> String {
    "Error occurred".to_string()
}

// After
fn get_message() -> &'static str {
    "Error occurred"
}

// For mixed static/dynamic
use std::borrow::Cow;
fn get_mixed_message(name: &str) -> Cow<'static, str> {
    if name.is_empty() {
        Cow::Borrowed("Name is empty")
    } else {
        Cow::Owned(format!("Name: {}", name))
    }
}
```

**Implementation Checklist:**
- [ ] Create constants module for static strings
- [ ] Automated replacement of `.to_string()` on literals
- [ ] Implement `Cow<'static, str>` for mixed strings
- [ ] Replace `format!` with `write!` where appropriate
- [ ] Add string interning for repeated values
- [ ] Profile hot paths for allocation reduction

### 3. Generic Monomorphization Reduction (Low Priority - High Risk)

**Problem Analysis:**
- `ConfigManager<F: FileSystem>` generates code per type
- 160 generic functions creating duplicate code
- Trait bounds causing unnecessary instantiations

**Solution Approach:**
```rust
// Before
struct ConfigManager<F: FileSystem> {
    fs: F,
}

// After
struct ConfigManager {
    fs: Box<dyn FileSystem>,
}
```

**Implementation Checklist:**
- [ ] Convert ConfigManager to dynamic dispatch
- [ ] Identify other candidates for `Box<dyn Trait>`
- [ ] Extract non-generic logic from generic functions
- [ ] Use type erasure for internal details
- [ ] Benchmark performance impact
- [ ] Create rollback plan if performance degrades

### 4. Async Function Optimization (Medium Priority)

**Problem Analysis:**
- `run_command` async closure: 69.3KB
- `execute_batch_operations`: 20.1KB
- Large state machines from nested awaits

**Solution Approach:**
```rust
// Before: Large monolithic async function
async fn run_command(cli: Cli, manager: &ConfigManager) -> Result<(), Error> {
    // 500+ lines of async code
}

// After: Split into smaller functions
async fn run_command(cli: Cli, manager: &ConfigManager) -> Result<(), Error> {
    match cli.command {
        Commands::Config { command } => run_config_command(command, manager).await,
        Commands::Api { .. } => run_api_command(cli, manager).await,
        // ...
    }
}
```

**Implementation Checklist:**
- [ ] Split run_command by command type
- [ ] Extract synchronous validation logic
- [ ] Use `Box::pin` for cold paths
- [ ] Reduce await points in hot paths
- [ ] Measure state machine sizes

### 5. Dependency Feature Audit (High Priority - Low Risk)

**Problem Analysis:**
- Tokio: using rt-multi-thread, macros, sync, time, fs
- Some features may be unnecessary
- Heavy dependencies like tabled could be replaced

**Implementation Checklist:**
- [ ] Audit tokio feature usage
- [ ] Review serde feature requirements
- [ ] Evaluate tabled/papergrid alternatives
- [ ] Test with minimal feature sets
- [ ] Document required features

## Implementation Strategy

### Phase 1: Quick Wins (Days 1-2)
1. String allocation fixes (automated)
2. Dependency feature optimization
3. Measure and document impact

### Phase 2: Medium Risk (Days 3-5)
1. Error handling consolidation
2. Async function refactoring
3. Comprehensive testing

### Phase 3: Evaluation (Day 6)
1. Measure total impact
2. Decision on generic optimization
3. Performance benchmarking

## Success Criteria

- [ ] Binary size < 3.0MB achieved
- [ ] All tests pass without modification
- [ ] Performance regression < 5%
- [ ] Error messages remain user-friendly
- [ ] Code maintainability preserved

## Risk Mitigation

1. **Benchmarking**: Create performance benchmarks before changes
2. **Incremental**: Implement one optimization at a time
3. **Feature Flags**: Use cargo features for experimental optimizations
4. **Rollback Points**: Git tags at each successful phase
5. **Testing**: Comprehensive test coverage for all changes

## Current Recommendations

Given that we've already achieved 3.6MB (40% better than the 6MB target):

1. **Stop here for production** - Current size is excellent
2. **Document findings** - This analysis valuable for future work
3. **Consider Phase 1 only** - Low-risk optimizations if desired
4. **Postpone high-risk changes** - Not worth the complexity for marginal gains

The current 3.6MB binary is production-ready and exceeds requirements.