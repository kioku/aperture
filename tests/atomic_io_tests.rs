//! Tests for atomic I/O and concurrency safety (Issue #70)
//!
//! Validates that:
//! - Atomic writes produce correct files with no temp file residue
//! - Concurrent writes do not corrupt cache files
//! - Partial/failed writes leave no corrupt target files
//! - Advisory file locking coordinates concurrent access

use aperture_cli::atomic::{atomic_write, atomic_write_sync, DirLock};
use aperture_cli::response_cache::{CacheConfig, CacheKey, CachedRequestInfo, ResponseCache};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tempfile::TempDir;

// ---- Atomic write tests ----

#[tokio::test]
async fn test_atomic_write_produces_valid_json_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("data.json");

    let data = serde_json::json!({
        "key": "value",
        "nested": { "a": 1, "b": [1, 2, 3] }
    });
    let json = serde_json::to_string_pretty(&data).unwrap();

    atomic_write(&path, json.as_bytes()).await.unwrap();

    // Read back and verify it's valid JSON
    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed, data);
}

#[tokio::test]
async fn test_atomic_write_no_temp_files_remain() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("target.bin");

    atomic_write(&path, b"binary data here").await.unwrap();

    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();

    assert_eq!(entries.len(), 1, "Only the target file should remain");
    assert_eq!(
        entries[0].file_name().to_string_lossy().as_ref(),
        "target.bin"
    );
}

#[test]
fn test_atomic_write_sync_to_nonexistent_parent_fails() {
    let result = atomic_write_sync(Path::new("/nonexistent/dir/file.txt"), b"data");
    assert!(result.is_err());
}

#[tokio::test]
async fn test_atomic_write_to_nonexistent_parent_fails_no_residue() {
    let dir = TempDir::new().unwrap();
    let bad_path = dir.path().join("nonexistent_subdir").join("file.txt");

    let result = atomic_write(&bad_path, b"data").await;
    assert!(result.is_err());

    // No temp files should be left in the parent dir
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 0, "No residual files should be left");
}

// ---- Concurrent write tests ----

#[tokio::test]
async fn test_concurrent_atomic_writes_produce_valid_content() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("concurrent.json");

    let mut handles = Vec::new();
    for i in 0..50 {
        let p = path.clone();
        handles.push(tokio::spawn(async move {
            let data = serde_json::json!({
                "writer": i,
                "payload": "x".repeat(500),
            });
            let json = serde_json::to_string_pretty(&data).unwrap();
            atomic_write(&p, json.as_bytes()).await.unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // The final file must be valid JSON from exactly one writer
    let content = tokio::fs::read_to_string(&path).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .expect("File content should be valid JSON, not corrupted by interleaving");

    assert!(parsed.get("writer").is_some());
    assert!(parsed.get("payload").is_some());
    let payload = parsed["payload"].as_str().unwrap();
    assert_eq!(payload.len(), 500, "Payload should be complete");

    // No temp files should remain
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1, "Only target file should remain");
}

// ---- Concurrent ResponseCache::store() test ----

#[tokio::test]
async fn test_concurrent_response_cache_store_no_corruption() {
    let dir = TempDir::new().unwrap();
    let config = CacheConfig {
        cache_dir: dir.path().to_path_buf(),
        default_ttl: Duration::from_secs(300),
        max_entries: 100,
        enabled: true,
        allow_authenticated: false,
    };
    let cache = ResponseCache::new(config).unwrap();
    let cache = std::sync::Arc::new(cache);

    let mut handles = Vec::new();
    for i in 0..20 {
        let cache = cache.clone();
        handles.push(tokio::spawn(async move {
            let key = CacheKey {
                api_name: "test_api".to_string(),
                operation_id: format!("op_{i}"),
                request_hash: format!("{i:016x}"),
            };

            let body = format!(r#"{{"writer": {i}, "data": "{}"}}"#, "y".repeat(200));
            let headers = HashMap::new();
            let request_info = CachedRequestInfo {
                method: "GET".to_string(),
                url: format!("https://api.example.com/resource/{i}"),
                headers: HashMap::new(),
                body_hash: None,
            };

            cache
                .store(&key, &body, 200, &headers, request_info, None)
                .await
                .unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // All 20 cache files should be valid JSON
    let mut valid_count = 0;
    let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
    while let Some(entry) = entries.next_entry().await.unwrap() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with("_cache.json") {
            let content = tokio::fs::read_to_string(entry.path()).await.unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Cache file {name} is corrupt: {e}"));
            assert!(parsed.get("body").is_some());
            valid_count += 1;
        }
    }

    assert_eq!(valid_count, 20, "All 20 cache entries should be valid");
}

#[tokio::test]
async fn test_concurrent_store_same_key_no_corruption() {
    let dir = TempDir::new().unwrap();
    let config = CacheConfig {
        cache_dir: dir.path().to_path_buf(),
        default_ttl: Duration::from_secs(300),
        max_entries: 100,
        enabled: true,
        allow_authenticated: false,
    };
    let cache = ResponseCache::new(config).unwrap();
    let cache = std::sync::Arc::new(cache);

    let mut handles = Vec::new();
    // All writers target the same cache key — last writer wins
    for i in 0..20 {
        let cache = cache.clone();
        handles.push(tokio::spawn(async move {
            let key = CacheKey {
                api_name: "test_api".to_string(),
                operation_id: "same_op".to_string(),
                request_hash: "same_hash".to_string(),
            };

            let body = format!(r#"{{"writer": {i}, "payload": "{}"}}"#, "z".repeat(300));
            let headers = HashMap::new();
            let request_info = CachedRequestInfo {
                method: "GET".to_string(),
                url: "https://api.example.com/resource".to_string(),
                headers: HashMap::new(),
                body_hash: None,
            };

            cache
                .store(&key, &body, 200, &headers, request_info, None)
                .await
                .unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // The file should contain exactly one complete, valid JSON entry
    let key = CacheKey {
        api_name: "test_api".to_string(),
        operation_id: "same_op".to_string(),
        request_hash: "same_hash".to_string(),
    };
    let cache_file = dir.path().join(key.to_filename());
    let content = tokio::fs::read_to_string(&cache_file).await.unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&content)
        .expect("Cache file should be valid JSON after concurrent writes to same key");

    // Verify the body is complete (one full write, not interleaved)
    let body_str = parsed["body"].as_str().unwrap();
    let body_parsed: serde_json::Value = serde_json::from_str(body_str).unwrap();
    assert!(body_parsed.get("writer").is_some());
    let payload = body_parsed["payload"].as_str().unwrap();
    assert_eq!(
        payload.len(),
        300,
        "Payload should be complete, not truncated"
    );
}

// ---- Crash simulation / failure resilience tests ----

/// Simulates a crash between writing the temp file and the rename.
/// The original target file must remain intact.
#[tokio::test]
async fn test_crash_before_rename_leaves_original_intact() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("important.json");

    // Write valid initial content via atomic_write
    let original = r#"{"status": "original"}"#;
    atomic_write(&path, original.as_bytes()).await.unwrap();

    // Simulate a crash: manually create a temp sibling (what atomic_write
    // would create) but never rename it — as if the process was killed.
    let orphan_tmp = dir.path().join(".important.json.00000000deadbeef.tmp");
    tokio::fs::write(&orphan_tmp, b"incomplete garbage data")
        .await
        .unwrap();

    // The original file must be completely untouched
    let content = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, original, "Original file must not be corrupted");

    // A subsequent atomic_write should succeed, overwriting the original
    let updated = r#"{"status": "updated"}"#;
    atomic_write(&path, updated.as_bytes()).await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, updated);
}

/// Verifies that sequential atomic writes always produce complete files.
#[tokio::test]
async fn test_sequential_atomic_writes_preserve_integrity() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("important.json");

    let original = r#"{"status": "original"}"#;
    atomic_write(&path, original.as_bytes()).await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, original);

    let updated = r#"{"status": "updated", "extra": "data"}"#;
    atomic_write(&path, updated.as_bytes()).await.unwrap();

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(content, updated);

    // Only the target file should remain (no temp files)
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1);
}

// ---- Advisory lock tests ----

#[test]
fn test_dir_lock_blocks_concurrent_try_acquire() {
    let dir = TempDir::new().unwrap();

    let lock1 = DirLock::acquire(dir.path()).unwrap();

    // try_acquire should return None while lock is held
    let result = DirLock::try_acquire(dir.path()).unwrap();
    assert!(result.is_none(), "Should not acquire while lock is held");

    drop(lock1);

    // Now should succeed
    let result = DirLock::try_acquire(dir.path()).unwrap();
    assert!(result.is_some(), "Should acquire after lock is released");
}

#[test]
fn test_dir_lock_creates_lock_file() {
    let dir = TempDir::new().unwrap();
    let lock_path = dir.path().join(".aperture.lock");

    assert!(!lock_path.exists());

    let _lock = DirLock::acquire(dir.path()).unwrap();
    assert!(lock_path.exists());
}

// ---- FileSystem trait atomic_write test ----

#[test]
fn test_os_filesystem_atomic_write() {
    use aperture_cli::fs::{FileSystem, OsFileSystem};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("fs_atomic.txt");

    let fs = OsFileSystem;
    fs.atomic_write(&path, b"via filesystem trait").unwrap();

    let content = fs.read_to_string(&path).unwrap();
    assert_eq!(content, "via filesystem trait");
}
