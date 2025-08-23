//! Shared test utilities for performance optimization

use once_cell::sync::Lazy;
use std::path::PathBuf;

/// Cached binary path for the aperture CLI to avoid repeated compilation
pub static APERTURE_BIN: Lazy<PathBuf> = Lazy::new(|| assert_cmd::cargo::cargo_bin("aperture"));

/// Test helper to create a command with the cached binary
pub fn aperture_cmd() -> assert_cmd::Command {
    assert_cmd::Command::new(&*APERTURE_BIN)
}
