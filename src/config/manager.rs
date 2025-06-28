use crate::error::Error;
use crate::fs::{FileSystem, OsFileSystem};
use openapiv3::OpenAPI;
use std::path::{Path, PathBuf};

pub struct ConfigManager<F: FileSystem> {
    fs: F,
    config_dir: PathBuf,
}

impl ConfigManager<OsFileSystem> {
    /// Creates a new `ConfigManager` with the default filesystem and config directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn new() -> Result<Self, Error> {
        let config_dir = get_config_dir()?;
        Ok(Self {
            fs: OsFileSystem,
            config_dir,
        })
    }
}

impl<F: FileSystem> ConfigManager<F> {
    pub const fn with_fs(fs: F, config_dir: PathBuf) -> Self {
        Self { fs, config_dir }
    }

    /// Adds a new `OpenAPI` specification to the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The spec already exists and `force` is false
    /// - File I/O operations fail
    /// - The `OpenAPI` spec is invalid YAML
    ///
    /// # Panics
    ///
    /// Panics if the spec path parent directory is None (should not happen in normal usage).
    pub fn add_spec(&self, name: &str, file_path: &Path, force: bool) -> Result<(), Error> {
        let spec_path = self.config_dir.join("specs").join(format!("{name}.yaml"));
        let _cache_path = self.config_dir.join(".cache").join(format!("{name}.bin"));

        if self.fs.exists(&spec_path) && !force {
            return Err(Error::Config(format!(
                "Spec '{name}' already exists. Use --force to overwrite."
            )));
        }

        let content = self.fs.read_to_string(file_path)?;
        let _openapi_spec: OpenAPI = serde_yaml::from_str(&content)?;

        // TODO: Implement validation against Aperture's supported feature set (SDD §5)
        // TODO: Transform into internal cached representation (from Task 2.4)
        // TODO: Serialize and write the cached representation to .cache/

        self.fs.create_dir_all(spec_path.parent().unwrap())?;
        self.fs.write_all(&spec_path, content.as_bytes())?;

        Ok(())
    }
}

/// Gets the default configuration directory path.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn get_config_dir() -> Result<PathBuf, Error> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| Error::Config("Could not determine home directory.".to_string()))?;
    let config_dir = home_dir.join(".config").join("aperture");
    Ok(config_dir)
}
