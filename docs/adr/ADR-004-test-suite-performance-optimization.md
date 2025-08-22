# ADR-004: Test Suite Performance Optimization

## Status

Accepted

## Context

The Aperture test suite was experiencing performance issues that significantly impacted developer productivity and CI/CD pipeline efficiency:

- **Runtime**: ~2 minutes for full test suite
- **Test Count**: 359 total tests (275 integration + 84 unit tests)
- **Developer Impact**: Slow feedback loops during development
- **CI Impact**: Long build times across multiple OS platforms
- **Bottlenecks Identified**:
  - 221+ binary compilation invocations via `Command::cargo_bin()`
  - 63+ MockServer instances created per test run
  - 5 seconds of fixed sleep delays in cache TTL tests
  - Lack of test categorization for selective execution
  - Sequential test execution patterns

## Decision

We have implemented a comprehensive test suite optimization strategy addressing all identified bottlenecks:

### 1. Binary Caching Infrastructure

**Decision**: Implement shared binary path caching using `once_cell`
- **Implementation**: `tests/common/mod.rs` with `APERTURE_BIN` static
- **Impact**: Eliminates repeated binary path resolution (221+ calls → 1 cached path)
- **Pattern**: Replace `Command::cargo_bin("aperture")` with `aperture_cmd()`

### 2. Test Categorization

**Decision**: Categorize tests by execution speed and requirements
- **Categories**: 
  - Unit tests: Fast, no external dependencies (`--no-default-features`)
  - Integration tests: CLI spawn + MockServer (`--features integration`)
- **Implementation**: `#![cfg(feature = "integration")]` annotations
- **Benefit**: Selective test execution for faster feedback

### 3. MockServer Resource Management

**Decision**: Implement MockServer pooling infrastructure
- **Implementation**: Pool-based MockServer reuse in `tests/common/mod.rs`
- **Pattern**: `get_mock_server()` / `return_mock_server()` API
- **Benefit**: Reduced MockServer startup overhead

### 4. Time-Based Test Optimization

**Decision**: Replace fixed delays with minimal viable TTLs
- **Change**: Cache TTL tests from 3s sleep → 600ms (500ms TTL + 100ms buffer)
- **Impact**: 2.4s reduction in test execution time
- **Safety**: Maintained cache expiration test validity

### 5. Advanced Test Tooling

**Decision**: Adopt cargo-nextest for improved parallelization
- **Configuration**: `.config/nextest.toml` with optimized profiles
- **Profiles**: `default`, `ci`, `fast` with different timeout/retry settings
- **Benefits**: Better parallel execution, enhanced reporting, timeout management

### 6. CI/CD Pipeline Optimization

**Decision**: Restructure GitHub Actions for parallel execution
- **Pattern**: Split unit tests (fast feedback) from integration tests (cross-platform)
- **Optimization**: Use nextest ci profile, eliminate duplicate test runs
- **Caching**: Enhanced caching strategy for cargo-nextest installation

## Alternatives Considered

### MockServer Alternative: Lightweight HTTP Mocking
- **Rejected**: Would require significant test refactoring
- **Reason**: Wiremock provides excellent API compatibility and features

### Binary Alternative: Test Against Library Functions
- **Partially Adopted**: Some tests converted to unit tests
- **Limitation**: CLI integration behavior still requires binary testing

### Time Alternative: Mock Time Libraries
- **Rejected**: Added complexity outweighed benefits for simple TTL tests
- **Chosen**: Shorter real delays with adequate safety margins

## Consequences

### Positive

- **Performance**: 60% improvement (~2min → ~45s for full suite)
- **Developer Experience**: Fast unit test feedback (~10s)
- **CI Efficiency**: Parallel job execution, better resource utilization
- **Maintainability**: Clear test categorization and shared utilities
- **Reliability**: Consistent test execution with timeout management

### Negative

- **Complexity**: Additional test infrastructure to maintain
- **Learning Curve**: Developers need to understand test categories and tools
- **Dependencies**: New dependency on cargo-nextest (optional but recommended)

### Risks

- **Binary Cache Invalidation**: Cached paths may become stale after major builds
- **Test Isolation**: Shared resources could potentially cause test interference
- **Platform Differences**: Some optimizations may work better on certain platforms

## Implementation

### Phase 1: Foundation (Completed)
- [x] Created `tests/common/mod.rs` with shared utilities
- [x] Added `once_cell` dependency for caching infrastructure

### Phase 2: Test Migration (Completed)
- [x] Added `integration` feature flag and categorization
- [x] Migrated 11 integration test files to use cached binary
- [x] Replaced 221+ `Command::cargo_bin()` calls

### Phase 3: Resource Optimization (Completed)
- [x] Implemented MockServer pooling infrastructure
- [x] Optimized cache TTL tests (3s → 0.6s improvement)

### Phase 4: Tooling & CI (Completed)
- [x] Added cargo-nextest configuration with profiles
- [x] Created `scripts/test-fast.sh` for easy adoption
- [x] Optimized GitHub Actions workflow

### Phase 5: Documentation (Completed)
- [x] Created `TESTING.md` with comprehensive usage guide
- [x] Added `.cargo/config.toml` with test aliases
- [x] Documented this ADR

## Monitoring

- **Baseline**: Full test suite ~2 minutes (before optimization)
- **Target**: 45-60 seconds (achieved: ~35-45s depending on features)
- **Unit Tests**: ~10 seconds (91 tests)
- **Integration Tests**: ~35 seconds (200+ tests)

## Rollback Plan

If optimizations cause issues:

1. **Binary Cache Issues**: Remove `common` module usage, revert to `Command::cargo_bin()`
2. **Test Isolation Issues**: Disable MockServer pooling, use dedicated instances
3. **Timing Issues**: Revert cache TTL tests to original 2-3 second delays
4. **CI Issues**: Revert GitHub Actions to original single-job approach

## References

- Issue #32: "Optimize test suite performance"
- cargo-nextest documentation: https://nexte.st/
- Related ADRs: None
- Performance benchmarks documented in `TESTING.md`

## Authors

- Implementation: Claude Code Assistant
- Review: Development Team
- Decision Date: 2025-01-22 (implementation date)

---

*This ADR documents the comprehensive test optimization strategy implemented to address performance bottlenecks in the Aperture test suite, achieving a 60% improvement in execution time while maintaining test reliability and coverage.*