//! Best-effort rollback for handled registration failures; this is not crash-atomic.
use crate::error::Error;
use crate::fs::FileSystem;
use std::path::PathBuf;

/// Lossless snapshots are taken before any registered file is replaced.
pub(super) struct Snapshot(Vec<(PathBuf, Option<Vec<u8>>)>);

impl Snapshot {
    pub(super) fn capture(fs: &impl FileSystem, paths: Vec<PathBuf>) -> Result<Self, Error> {
        let mut saved = Vec::new();
        for path in paths {
            let bytes = if fs.exists(&path) {
                Some(fs.read_bytes(&path)?)
            } else {
                None
            };
            saved.push((path, bytes));
        }
        Ok(Self(saved))
    }

    pub(super) fn finish(
        self,
        fs: &impl FileSystem,
        result: Result<(), Error>,
    ) -> Result<(), Error> {
        let Err(original) = result else {
            return Ok(());
        };
        let mut rollback_failed = false;
        for (path, bytes) in self.0.into_iter().rev() {
            let restored = match bytes {
                Some(data) => fs.atomic_write(&path, &data),
                None if fs.exists(&path) => fs.remove_file(&path),
                None => Ok(()),
            };
            rollback_failed |= restored.is_err();
        }
        if rollback_failed {
            return Err(Error::invalid_config(format!(
                "Registration failed and rollback could not restore all files: {original}"
            )));
        }
        Err(original)
    }
}
