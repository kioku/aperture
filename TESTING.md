# Testing Guide

This document describes the optimized testing setup for the Aperture project.

## Test Performance Optimizations

The test suite has been optimized for speed and efficiency:

- **Binary Caching**: Test utilities use cached binary paths to avoid repeated compilation
- **Test Categorization**: Tests are categorized as unit vs integration for selective execution
- **MockServer Pooling**: HTTP mock servers are reused to reduce startup overhead
- **Optimized Delays**: Cache TTL tests use minimal delays (600ms vs 3s)
- **Parallel Execution**: Tests are configured for optimal parallel execution

## Running Tests

### Quick Commands

```bash
# Run all unit tests (fast, ~10s)
cargo test --no-default-features

# Run all integration tests (~35s) 
cargo test --features integration

# Run specific test file
cargo test --features integration --test integration_tests

# Run with nextest (recommended for development)
./scripts/test-fast.sh
```

### Test Categories

- **Unit Tests**: Fast, isolated tests without external dependencies
- **Integration Tests**: End-to-end tests that spawn the CLI binary and use MockServer

### Using cargo-nextest (Recommended)

cargo-nextest provides better parallelization and reporting:

```bash
# Install nextest (one-time setup)
cargo install cargo-nextest --locked

# Run with nextest profiles
cargo nextest run --profile fast        # Fast local development
cargo nextest run --profile default     # Standard configuration  
cargo nextest run --profile ci          # CI optimized

# Or use the provided script
./scripts/test-fast.sh
```

## Test Configuration

### Nextest Configuration

The `.config/nextest.toml` file contains:
- Optimized thread counts and timeouts
- Different profiles for local development vs CI
- Special handling for integration tests
- JUnit output for CI integration

### Cargo Configuration  

The `.cargo/config.toml` file provides:
- Build optimizations for faster compilation
- Test aliases for common workflows
- Environment variable defaults

## Performance Benchmarks

Current test performance (as of optimization):

| Test Category | Count | Runtime | Notes |
|---------------|-------|---------|-------|
| Unit Tests | 91 | ~10s | No integration feature |
| Integration Tests | ~200 | ~35s | With integration feature |
| Total Suite | ~291 | ~45s | All tests enabled |

Previous performance was ~2 minutes, representing a ~60% improvement.

## Development Workflow

1. **Local Development**: Use `cargo test --no-default-features` for quick feedback
2. **Feature Testing**: Use `cargo test --features integration` when testing CLI features
3. **Pre-commit**: Run full suite with `./scripts/test-fast.sh` or `cargo test --features integration`
4. **CI**: Automatically runs with nextest ci profile

## Troubleshooting

### Slow Tests
- Check if you're running integration tests unnecessarily
- Use `cargo test --no-default-features` for unit tests only
- Consider using nextest for better parallelization

### Test Failures
- Integration tests require the `integration` feature flag
- Some tests depend on timing and may be flaky in very slow environments
- MockServer tests may conflict if ports are in use

### Binary Cache Issues
- If tests fail with binary not found, the cached path may be stale
- Clean and rebuild: `cargo clean && cargo build`