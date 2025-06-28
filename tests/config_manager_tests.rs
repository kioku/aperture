use aperture::config::manager::ConfigManager;
use aperture::error::Error;
use aperture::fs::FileSystem;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// Mock FileSystem implementation for testing
#[derive(Clone)]
pub struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    dirs: Arc<Mutex<HashMap<PathBuf, bool>>>,
    io_error_on_read: Arc<Mutex<bool>>,
    io_error_on_write: Arc<Mutex<bool>>,
}

impl MockFileSystem {
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            dirs: Arc::new(Mutex::new(HashMap::new())),
            io_error_on_read: Arc::new(Mutex::new(false)),
            io_error_on_write: Arc::new(Mutex::new(false)),
        }
    }

    pub fn set_io_error_on_read(&self, value: bool) {
        *self.io_error_on_read.lock().unwrap() = value;
    }

    pub fn set_io_error_on_write(&self, value: bool) {
        *self.io_error_on_write.lock().unwrap() = value;
    }

    pub fn add_file(&self, path: &Path, content: &str) {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), content.as_bytes().to_vec());
        self.dirs
            .lock()
            .unwrap()
            .insert(path.parent().unwrap().to_path_buf(), true);
    }

    pub fn add_dir(&self, path: &Path) {
        self.dirs.lock().unwrap().insert(path.to_path_buf(), true);
    }

    pub fn get_file_content(&self, path: &Path) -> Option<String> {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .map(|v| String::from_utf8_lossy(v).to_string())
    }
}

impl FileSystem for MockFileSystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if *self.io_error_on_read.lock().unwrap() {
            return Err(io::Error::new(ErrorKind::Other, "Mock I/O error on read"));
        }
        self.files
            .lock()
            .unwrap()
            .get(path)
            .map(|v| String::from_utf8_lossy(v).to_string())
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "File not found"))
    }

    fn write_all(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if *self.io_error_on_write.lock().unwrap() {
            return Err(io::Error::new(ErrorKind::Other, "Mock I/O error on write"));
        }
        self.files
            .lock()
            .unwrap()
            .insert(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dirs.lock().unwrap().insert(path.to_path_buf(), true);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "File not found"))
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        files.retain(|p, _| !p.starts_with(path));
        let mut dirs = self.dirs.lock().unwrap();
        dirs.retain(|p, _| !p.starts_with(path));
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
            || self.dirs.lock().unwrap().contains_key(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.lock().unwrap().contains_key(path)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let files = self.files.lock().unwrap();
        let dirs = self.dirs.lock().unwrap();
        let mut entries = Vec::new();
        for (p, _) in files.iter() {
            if p.parent() == Some(path) {
                entries.push(p.clone());
            }
        }
        for (p, _) in dirs.iter() {
            if p.parent() == Some(path) && p != path {
                entries.push(p.clone());
            }
        }
        Ok(entries)
    }
}

// --- Tests for ConfigManager ---

const TEST_CONFIG_DIR: &str = "/tmp/aperture_test_config";

fn setup_manager() -> (ConfigManager<MockFileSystem>, MockFileSystem) {
    let fs = MockFileSystem::new();
    let config_dir = PathBuf::from(TEST_CONFIG_DIR);
    fs.add_dir(&config_dir);
    fs.add_dir(&config_dir.join("specs"));
    fs.add_dir(&config_dir.join(".cache"));
    let manager = ConfigManager::with_fs(fs.clone(), config_dir);
    (manager, fs)
}

#[test]
fn test_add_spec_new() {
    let (manager, fs) = setup_manager();
    let spec_name = "my-new-api";
    let spec_content = r#"
openapi: 3.0.0
info:
  title: My New API
  version: 1.0.0
paths: {}
"#;
    let temp_spec_path = PathBuf::from("/tmp/new_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(spec_name, &temp_spec_path, false);
    assert!(result.is_ok());

    let expected_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("my-new-api.yaml");
    assert!(fs.exists(&expected_path));
    assert_eq!(fs.get_file_content(&expected_path).unwrap(), spec_content);
}

#[test]
fn test_add_spec_exists_no_force() {
    let (manager, fs) = setup_manager();
    let spec_name = "existing-api";
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Existing API
  version: 1.0.0
paths: {}
"#;
    let existing_spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("existing-api.yaml");
    fs.add_file(&existing_spec_path, spec_content);

    let temp_spec_path = PathBuf::from("/tmp/updated_api.yaml");
    fs.add_file(&temp_spec_path, "updated content");

    let result = manager.add_spec(spec_name, &temp_spec_path, false);
    assert!(result.is_err());
    if let Err(Error::Config(msg)) = result {
        assert!(msg.contains("already exists"));
    } else {
        panic!("Unexpected error type");
    }
    // Ensure content was not overwritten
    assert_eq!(
        fs.get_file_content(&existing_spec_path).unwrap(),
        spec_content
    );
}

#[test]
fn test_add_spec_exists_with_force() {
    let (manager, fs) = setup_manager();
    let spec_name = "existing-api";
    let original_content = r#"
openapi: 3.0.0
info:
  title: Existing API
  version: 1.0.0
paths: {}
"#;
    let existing_spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("existing-api.yaml");
    fs.add_file(&existing_spec_path, original_content);

    let updated_content = r#"
openapi: 3.0.0
info:
  title: Updated API
  version: 2.0.0
paths: {}
"#;
    let temp_spec_path = PathBuf::from("/tmp/updated_api.yaml");
    fs.add_file(&temp_spec_path, updated_content);

    let result = manager.add_spec(spec_name, &temp_spec_path, true);
    assert!(result.is_ok());

    assert_eq!(
        fs.get_file_content(&existing_spec_path).unwrap(),
        updated_content
    );
}

#[test]
fn test_add_spec_invalid_openapi() {
    let (manager, fs) = setup_manager();
    let spec_name = "invalid-api";
    let invalid_content = "not a valid openapi yaml";
    let temp_spec_path = PathBuf::from("/tmp/invalid_api.yaml");
    fs.add_file(&temp_spec_path, invalid_content);

    let result = manager.add_spec(spec_name, &temp_spec_path, false);
    assert!(result.is_err());
    if let Err(Error::Yaml(err)) = result {
        assert!(err.to_string().contains("invalid type: string"));
    } else {
        panic!("Unexpected error type: {:?}", result);
    }
}

#[test]
fn test_add_spec_io_error_on_read() {
    let (manager, fs) = setup_manager();
    let spec_name = "io-error-api";
    let temp_spec_path = PathBuf::from("/tmp/io_error_api.yaml");
    fs.add_file(&temp_spec_path, "dummy content");
    fs.set_io_error_on_read(true);

    let result = manager.add_spec(spec_name, &temp_spec_path, false);
    assert!(result.is_err());
    if let Err(Error::Io(err)) = result {
        assert!(err.to_string().contains("Mock I/O error on read"));
    } else {
        panic!("Unexpected error type: {:?}", result);
    }
}

#[test]
fn test_add_spec_io_error_on_write() {
    let (manager, fs) = setup_manager();
    let spec_name = "io-error-api";
    let spec_content = r#"
openapi: 3.0.0
info:
  title: My New API
  version: 1.0.0
paths: {}
"#;
    let temp_spec_path = PathBuf::from("/tmp/io_error_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);
    fs.set_io_error_on_write(true);

    let result = manager.add_spec(spec_name, &temp_spec_path, false);
    assert!(result.is_err());
    if let Err(Error::Io(err)) = result {
        assert!(err.to_string().contains("Mock I/O error on write"));
    } else {
        panic!("Unexpected error type: {:?}", result);
    }
}

#[test]
fn test_list_specs_empty_dir() {
    let (manager, _fs) = setup_manager();
    let specs = manager.list_specs().unwrap();
    assert!(specs.is_empty());
}

#[test]
fn test_list_specs_multiple_specs() {
    let (manager, fs) = setup_manager();
    let specs_dir = PathBuf::from(TEST_CONFIG_DIR).join("specs");
    fs.add_file(&specs_dir.join("api1.yaml"), "content");
    fs.add_file(&specs_dir.join("api2.yaml"), "content");
    fs.add_file(&specs_dir.join("api3.json"), "content"); // Should be ignored
    fs.add_dir(&specs_dir.join("subdir")); // Should be ignored

    let mut specs = manager.list_specs().unwrap();
    specs.sort();

    assert_eq!(specs, vec!["api1".to_string(), "api2".to_string()]);
}

#[test]
fn test_list_specs_no_specs_dir() {
    let fs = MockFileSystem::new();
    let config_dir = PathBuf::from(TEST_CONFIG_DIR);
    // Do not add specs directory
    let manager = ConfigManager::with_fs(fs.clone(), config_dir);

    let specs = manager.list_specs().unwrap();
    assert!(specs.is_empty());
}
