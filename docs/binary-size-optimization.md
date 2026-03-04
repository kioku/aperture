# Binary Size Optimization

## Overview

This document tracks the binary size history and optimization strategies for Aperture CLI.

## Size History

| Version / Change | Binary Size | Notes |
|------------------|-------------|-------|
| Original (unoptimized) | 11 MB | Debug build, no profile tuning |
| + Compiler optimizations | 4.0 MB | Release profile: `opt-level = "z"`, LTO, strip |
| + native-tls → reqwest 0.12 default | 3.6 MB | Removed bundled OpenSSL |
| reqwest 0.12 → 0.13 (aws-lc-rs) | 7.6 MB | aws-lc-rs became default rustls provider; aws-lc-sys contributes ~1.2 MB of compiled C |
| + Platform-conditional TLS (current) | **5.8 MB** | ring on non-Windows eliminates aws-lc-sys |

## Applied Optimizations

### 1. Compiler Release Profile

```toml
[profile.release]
strip = true          # Strip symbols
opt-level = "z"       # Optimize for size over speed
lto = "fat"          # Link-time optimization
codegen-units = 1    # Single codegen unit
panic = "abort"      # Smaller panic handler
```

### 2. Platform-Conditional TLS Crypto Provider

The `aws-lc-rs` rustls crypto provider bundles `aws-lc-sys`, a compiled C library that
contributes ~1.2 MB to the `.text` section on Linux/macOS. Switching to `ring` (pure
Rust/ASM) on non-Windows platforms eliminates this entirely.

`ring` requires NASM at build time on `x86_64-pc-windows-msvc`, so `aws-lc-rs` is
retained on Windows.

```toml
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-no-provider"] }

[target.'cfg(not(windows))'.dependencies]
rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12", "logging"] }

[target.'cfg(windows)'.dependencies]
rustls = { version = "0.23", default-features = false, features = ["aws_lc_rs", "std", "tls12", "logging"] }
```

The provider must be installed before any `reqwest::Client` is built — reqwest 0.13's
`ClientBuilder::build()` checks `CryptoProvider::get_default()` first, and if nothing is
installed, falls back to a hardcoded aws-lc-rs path that panics when the feature is absent.

```rust
// src/main.rs
#[cfg(not(windows))]
let _ = rustls::crypto::ring::default_provider().install_default();
#[cfg(windows)]
let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
```

### 3. Build Profiles

```bash
# Default release (5.8 MB on Linux x86_64)
cargo build --release

# Minimal profile — same flags, explicit alias
cargo build --profile minimal

# Size-optimized — trades some size for faster LTO
cargo build --profile size-optimized
```

## Measurement Tools

```bash
# Check binary size
ls -lh target/release/aperture

# Verify aws-lc-sys is absent from the dependency tree (non-Windows)
cargo tree | grep aws-lc-sys

# Analyze crate contributions (requires cargo-bloat)
cargo install cargo-bloat
cargo bloat --release --crates
```

## Trade-offs

| Optimization | Cost |
|---|---|
| `opt-level = "z"` | ~10–20% slower execution vs default |
| `lto = "fat"` | +30–60 s compile time |
| `codegen-units = 1` | Slower parallel compilation |
| `panic = "abort"` | No stack unwinding on panic |
| ring (non-Windows) | No FIPS compliance; aws-lc-rs required for FIPS targets |

## Remaining Opportunities

The following were analyzed but not yet implemented. Risk/reward is low enough that
they are deferred:

| Area | Potential Saving | Risk |
|------|-----------------|------|
| Error enum consolidation (50+ variants → ~10 kinds) | 300–500 KB | Medium |
| String allocation reduction (1,513 `.to_string()` sites) | 100–200 KB | Low |
| Generic monomorphization (`ConfigManager<F>` → `dyn FileSystem`) | 100–300 KB | High |
| Async state machine splitting (`run_command` is 69 KB) | 50–150 KB | Medium |
