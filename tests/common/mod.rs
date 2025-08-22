//! Shared test utilities for performance optimization

use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use wiremock::MockServer;

/// Cached binary path for the aperture CLI to avoid repeated compilation
pub static APERTURE_BIN: Lazy<PathBuf> = Lazy::new(|| assert_cmd::cargo::cargo_bin("aperture"));

/// Mock server pool to reuse servers across tests
static MOCK_SERVER_POOL: Lazy<Arc<Mutex<Vec<MockServer>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

/// Get a mock server from the pool or create a new one
pub async fn get_mock_server() -> MockServer {
    {
        let mut pool = MOCK_SERVER_POOL.lock().unwrap();
        if let Some(server) = pool.pop() {
            return server;
        }
    }
    MockServer::start().await
}

/// Return a mock server to the pool for reuse
pub fn return_mock_server(server: MockServer) {
    let mut pool = MOCK_SERVER_POOL.lock().unwrap();
    // Limit pool size to prevent memory growth
    if pool.len() < 5 {
        pool.push(server);
    }
}

/// Shared temporary directory manager
static TEMP_DIR_POOL: Lazy<Arc<Mutex<Vec<TempDir>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

/// Get a temporary directory from the pool or create a new one
pub fn get_temp_dir() -> std::io::Result<TempDir> {
    {
        let mut pool = TEMP_DIR_POOL.lock().unwrap();
        if let Some(temp_dir) = pool.pop() {
            return Ok(temp_dir);
        }
    }
    TempDir::new()
}

/// Return a temporary directory to the pool for reuse
pub fn return_temp_dir(temp_dir: TempDir) {
    let mut pool = TEMP_DIR_POOL.lock().unwrap();
    // Limit pool size to prevent memory growth
    if pool.len() < 3 {
        pool.push(temp_dir);
    }
}

/// Test helper to create a command with the cached binary
pub fn aperture_cmd() -> assert_cmd::Command {
    assert_cmd::Command::new(&*APERTURE_BIN)
}

/// Cleanup function to be called at the end of test suites
#[allow(dead_code)]
pub fn cleanup_pools() {
    // Clear all pools to free resources
    MOCK_SERVER_POOL.lock().unwrap().clear();
    TEMP_DIR_POOL.lock().unwrap().clear();
}
