//! Atomic file I/O utilities for concurrency-safe cache operations.
//!
//! This module provides:
//! - **Atomic writes** via temp-file + rename to prevent partial/corrupt files.
//! - **Advisory file locking** for coordinating concurrent access to cache directories.
//!
//! # Concurrency Guarantees
//!
//! - A reader will never see a partially written file.
//! - Concurrent writers to the same path will not interleave bytes; the last
//!   rename wins, producing one complete file.
//! - Advisory locks coordinate cache-directory operations across processes.
//!
//! # Cross-Platform Notes
//!
//! - On POSIX systems, `rename(2)` is atomic within the same filesystem.
//! - On Windows, `std::fs::rename` uses `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`,
//!   which is atomic for same-volume renames.

use std::path::Path;

/// Write `data` to `path` atomically by writing to a temporary sibling file
/// and then renaming it into place.
///
/// The temp file is created in the same directory as `path` to guarantee
/// same-filesystem rename semantics.
///
/// # Errors
///
/// Returns an error if:
/// - The parent directory of `path` does not exist.
/// - The temp file cannot be created or written.
/// - The rename operation fails.
pub async fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temp_path = temp_sibling(path);

    // Write data to temp file
    tokio::fs::write(&temp_path, data).await?;

    // Atomically move temp file to target
    if let Err(e) = tokio::fs::rename(&temp_path, path).await {
        // Clean up the temp file on rename failure
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(e);
    }

    Ok(())
}

/// Synchronous version of [`atomic_write`] for use in contexts that cannot
/// use async (e.g., the [`FileSystem`](crate::fs::FileSystem) trait).
///
/// # Errors
///
/// Returns an error if any file operation fails.
pub fn atomic_write_sync(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let temp_path = temp_sibling(path);

    // Write data to temp file
    std::fs::write(&temp_path, data)?;

    // Atomically move temp file to target
    if let Err(e) = std::fs::rename(&temp_path, path) {
        // Clean up the temp file on rename failure
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    Ok(())
}

/// Generate a unique temporary file path as a sibling of `path`.
///
/// Uses `fastrand` for a random suffix to avoid collisions between
/// concurrent writers targeting the same destination.
fn temp_sibling(path: &Path) -> std::path::PathBuf {
    let random_suffix = fastrand::u64(..);
    let file_name = path
        .file_name()
        .map_or_else(|| "file".to_string(), |n| n.to_string_lossy().to_string());

    let temp_name = format!(".{file_name}.{random_suffix:016x}.tmp");

    path.with_file_name(temp_name)
}

/// Check whether an I/O error represents a lock-contention condition
/// on the current platform.
fn is_lock_contention_error(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        // EAGAIN and EWOULDBLOCK are the same value on Linux but may
        // differ on other POSIX systems, so we check both.
        let code = e.raw_os_error();
        code == Some(libc::EAGAIN) || code == Some(libc::EWOULDBLOCK)
    }
    #[cfg(windows)]
    {
        // ERROR_LOCK_VIOLATION = 33
        e.raw_os_error() == Some(33)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = e;
        false
    }
}

/// Name of the advisory lock file placed in cache directories.
const LOCK_FILE_NAME: &str = ".aperture.lock";

/// An advisory file lock scoped to a directory.
///
/// The lock is acquired on creation and released when the guard is dropped.
/// This uses `fs2` advisory locking which coordinates between cooperating
/// processes — it does **not** prevent non-cooperating processes from
/// accessing the directory.
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use aperture_cli::atomic::DirLock;
///
/// let lock = DirLock::acquire(Path::new("/tmp/cache")).unwrap();
/// // … perform cache operations …
/// drop(lock); // lock is released
/// ```
pub struct DirLock {
    _file: std::fs::File,
}

impl DirLock {
    /// Acquire an exclusive advisory lock on `dir`.
    ///
    /// Creates the lock file (`<dir>/.aperture.lock`) if it does not exist.
    /// Blocks until the lock is available.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock file cannot be created or locked.
    pub fn acquire(dir: &Path) -> std::io::Result<Self> {
        use fs2::FileExt;

        let lock_path = dir.join(LOCK_FILE_NAME);

        // Ensure the directory exists
        std::fs::create_dir_all(dir)?;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;

        file.lock_exclusive()?;

        Ok(Self { _file: file })
    }

    /// Try to acquire an exclusive advisory lock without blocking.
    ///
    /// Returns `Ok(None)` if the lock is held by another process.
    ///
    /// # Errors
    ///
    /// Returns an error if the lock file cannot be created.
    pub fn try_acquire(dir: &Path) -> std::io::Result<Option<Self>> {
        use fs2::FileExt;

        let lock_path = dir.join(LOCK_FILE_NAME);

        // Ensure the directory exists
        std::fs::create_dir_all(dir)?;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => {
                // On some platforms, `try_lock_exclusive` may return a
                // platform-specific error code instead of `WouldBlock`.
                // Only treat known lock-contention codes as "already held".
                if is_lock_contention_error(&e) {
                    return Ok(None);
                }
                Err(e)
            }
        }
    }
}

// The lock is released when `_file` is dropped — `fs2` advisory locks
// are automatically released when the file descriptor is closed.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_atomic_write_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, b"hello world").await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_atomic_write_no_temp_files_left() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, b"data").await.unwrap();

        // Only the target file should exist
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].file_name().to_string_lossy().as_ref(),
            "test.txt"
        );
    }

    #[tokio::test]
    async fn test_atomic_write_overwrites_existing() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, b"first").await.unwrap();
        atomic_write(&path, b"second").await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, "second");
    }

    #[test]
    fn test_atomic_write_sync_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write_sync(&path, b"hello sync").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello sync");
    }

    #[test]
    fn test_atomic_write_sync_no_temp_files_left() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write_sync(&path, b"data").unwrap();

        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_dir_lock_acquire_and_release() {
        let dir = TempDir::new().unwrap();

        let lock = DirLock::acquire(dir.path()).unwrap();
        // Lock file should exist
        assert!(dir.path().join(LOCK_FILE_NAME).exists());

        drop(lock);
        // Lock file still exists (we don't delete it) but lock is released
        assert!(dir.path().join(LOCK_FILE_NAME).exists());
    }

    #[test]
    fn test_dir_lock_try_acquire() {
        let dir = TempDir::new().unwrap();

        let lock1 = DirLock::try_acquire(dir.path()).unwrap();
        assert!(lock1.is_some());

        // Second try-acquire should fail while first lock is held
        let lock2 = DirLock::try_acquire(dir.path()).unwrap();
        assert!(lock2.is_none());

        // After dropping first lock, try-acquire should succeed
        drop(lock1);
        let lock3 = DirLock::try_acquire(dir.path()).unwrap();
        assert!(lock3.is_some());
    }

    #[tokio::test]
    async fn test_concurrent_atomic_writes_no_corruption() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.txt");

        let mut handles = Vec::new();
        for i in 0..20 {
            let p = path.clone();
            handles.push(tokio::spawn(async move {
                let data = format!("writer-{i}-{}", "x".repeat(1000));
                atomic_write(&p, data.as_bytes()).await.unwrap();
            }));
        }

        for handle in handles {
            handle.await.unwrap();
        }

        // The file should contain one complete write — not a mixture
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.starts_with("writer-"));
        assert!(content.ends_with(&"x".repeat(1000)));
    }

    #[test]
    fn test_temp_sibling_uniqueness() {
        let path = Path::new("/tmp/cache/test.json");
        let t1 = temp_sibling(path);
        let t2 = temp_sibling(path);
        // Should be in the same directory
        assert_eq!(t1.parent(), t2.parent());
        assert_eq!(t1.parent().unwrap(), Path::new("/tmp/cache"));
        // Should start with dot (hidden)
        let name1 = t1.file_name().unwrap().to_string_lossy();
        assert!(name1.starts_with('.'));
        assert!(name1.ends_with(".tmp"));
        // Names should (almost certainly) be different due to random suffix
        assert_ne!(t1, t2);
    }
}
