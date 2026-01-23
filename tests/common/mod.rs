//! Shared test utilities for performance optimization

use std::path::PathBuf;

/// Cached binary path for the aperture CLI to avoid repeated compilation
#[allow(deprecated)] // TODO: Migrate to cargo_bin! macro when LazyLock-compatible
pub static APERTURE_BIN: std::sync::LazyLock<PathBuf> =
    std::sync::LazyLock::new(|| assert_cmd::cargo::cargo_bin("aperture"));

/// Test helper to create a command with the cached binary
pub fn aperture_cmd() -> assert_cmd::Command {
    assert_cmd::Command::new(&*APERTURE_BIN)
}
