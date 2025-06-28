use std::io;
use std::path::{Path, PathBuf};

pub trait FileSystem {
    /// Reads the entire contents of a file into a string.
    ///
    /// # Errors
    ///
    /// Returns an error if the file does not exist, cannot be read, or contains invalid UTF-8.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Writes a slice of bytes to a file, creating the file if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be written to or created.
    fn write_all(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Creates a directory and all of its parent components if they are missing.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Removes a file from the filesystem.
    ///
    /// # Errors
    ///
    /// Returns an error if the file does not exist or cannot be removed.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Removes a directory and all of its contents.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory does not exist or cannot be removed.
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Returns `true` if the path points to an existing entity.
    fn exists(&self, path: &Path) -> bool;

    /// Returns `true` if the path exists and is pointing at a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Returns `true` if the path exists and is pointing at a regular file.
    fn is_file(&self, path: &Path) -> bool;

    /// Returns the canonical, absolute form of the path with all intermediate components normalized.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist or cannot be canonicalized.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// Returns a vector of all entries in a directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory does not exist or cannot be read.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;
}

pub struct OsFileSystem;

impl FileSystem for OsFileSystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write_all(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .collect())
    }
}
