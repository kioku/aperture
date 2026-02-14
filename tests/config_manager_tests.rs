// These lints are overly pedantic for test code
#![allow(clippy::default_trait_access)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::used_underscore_binding)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::significant_drop_tightening)]

mod test_helpers;

use aperture_cli::config::context_name::ApiContextName;
use aperture_cli::config::manager::{is_url, ConfigManager};
use aperture_cli::error::{Error, ErrorKind};
use aperture_cli::fs::FileSystem;

/// Helper to create a validated `ApiContextName` from a string literal in tests
fn name(s: &str) -> ApiContextName {
    ApiContextName::new(s).expect("test name should be valid")
}
use std::collections::HashMap;
use std::io;
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

impl Default for MockFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl MockFileSystem {
    #[must_use]
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

    #[must_use]
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
            return Err(io::Error::other("Mock I/O error on read"));
        }
        self.files
            .lock()
            .unwrap()
            .get(path)
            .map(|v| String::from_utf8_lossy(v).to_string())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))
    }

    fn write_all(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if *self.io_error_on_write.lock().unwrap() {
            return Err(io::Error::other("Mock I/O error on write"));
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
        if *self.io_error_on_write.lock().unwrap() {
            return Err(io::Error::other("Mock I/O error on write"));
        }
        self.files
            .lock()
            .unwrap()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "File not found"))
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

    fn atomic_write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.write_all(path, contents)
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

const fn create_minimal_spec() -> &'static str {
    r"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
paths:
  /test:
    get:
      responses:
        '200':
          description: Success
"
}

#[test]
fn test_add_spec_new() {
    let (manager, fs) = setup_manager();
    let spec_name = "my-new-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: My New API
  version: 1.0.0
paths: {}
";
    let temp_spec_path = PathBuf::from("/tmp/new_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
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
    let spec_content = r"
openapi: 3.0.0
info:
  title: Existing API
  version: 1.0.0
paths: {}
";
    let existing_spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("existing-api.yaml");
    fs.add_file(&existing_spec_path, spec_content);

    let temp_spec_path = PathBuf::from("/tmp/updated_api.yaml");
    fs.add_file(&temp_spec_path, "updated content");

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind,
            message,
            context,
        }) => {
            assert_eq!(kind, ErrorKind::Specification);
            assert!(message.contains("already exists"));
            assert!(message.contains(spec_name));
            let Some(ctx) = context else { return };
            let Some(details) = &ctx.details else { return };
            assert_eq!(details["spec_name"], spec_name);
        }
        _ => panic!("Unexpected error type: {result:?}"),
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
    let original_content = r"
openapi: 3.0.0
info:
  title: Existing API
  version: 1.0.0
paths: {}
";
    let existing_spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("existing-api.yaml");
    fs.add_file(&existing_spec_path, original_content);

    let updated_content = r"
openapi: 3.0.0
info:
  title: Updated API
  version: 2.0.0
paths: {}
";
    let temp_spec_path = PathBuf::from("/tmp/updated_api.yaml");
    fs.add_file(&temp_spec_path, updated_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, true, true);
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

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Yaml(err)) = result {
        assert!(err.to_string().contains("invalid type: string"));
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_io_error_on_read() {
    let (manager, fs) = setup_manager();
    let spec_name = "io-error-api";
    let temp_spec_path = PathBuf::from("/tmp/io_error_api.yaml");
    fs.add_file(&temp_spec_path, "dummy content");
    fs.set_io_error_on_read(true);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Io(err)) = result {
        assert!(err.to_string().contains("Mock I/O error on read"));
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_io_error_on_write() {
    let (manager, fs) = setup_manager();
    let spec_name = "io-error-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: My New API
  version: 1.0.0
paths: {}
";
    let temp_spec_path = PathBuf::from("/tmp/io_error_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);
    fs.set_io_error_on_write(true);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Io(err)) = result {
        assert!(err.to_string().contains("Mock I/O error on write"));
    } else {
        panic!("Unexpected error type: {result:?}");
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
    let manager = ConfigManager::with_fs(fs, config_dir);

    let specs = manager.list_specs().unwrap();
    assert!(specs.is_empty());
}

#[test]
fn test_remove_spec_success() {
    let (manager, fs) = setup_manager();
    let spec_name = "to-remove-api";
    let spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("to-remove-api.yaml");
    let cache_path = PathBuf::from(TEST_CONFIG_DIR)
        .join(".cache")
        .join("to-remove-api.bin");
    fs.add_file(&spec_path, "content");
    fs.add_file(&cache_path, "cached content");

    let result = manager.remove_spec(&name(spec_name));
    assert!(result.is_ok());
    assert!(!fs.exists(&spec_path));
    assert!(!fs.exists(&cache_path));
}

#[test]
fn test_remove_spec_not_found() {
    let (manager, _fs) = setup_manager();
    let spec_name = "non-existent-api";

    let result = manager.remove_spec(&name(spec_name));
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind,
            message,
            context,
        }) => {
            assert_eq!(kind, ErrorKind::Specification);
            assert!(message.contains("not found"));
            assert!(message.contains(spec_name));
            let Some(ctx) = context else { return };
            let Some(details) = &ctx.details else { return };
            assert_eq!(details["spec_name"], spec_name);
        }
        _ => panic!("Unexpected error type: {result:?}"),
    }
}

#[test]
fn test_remove_spec_io_error() {
    let (manager, fs) = setup_manager();
    let spec_name = "io-error-remove-api";
    let spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("io-error-remove-api.yaml");
    fs.add_file(&spec_path, "content");
    fs.set_io_error_on_write(true); // Simulate I/O error on remove

    let result = manager.remove_spec(&name(spec_name));
    assert!(result.is_err());
    if let Err(Error::Io(err)) = result {
        assert!(err.to_string().contains("Mock I/O error on write"));
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

// --- Tests for OpenAPI validation and caching ---

#[test]
fn test_add_spec_with_valid_api_key_security() {
    let (manager, fs) = setup_manager();
    let spec_name = "api-key-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: API Key API
  version: 1.0.0
components:
  securitySchemes:
    apiKey:
      type: apiKey
      in: header
      name: X-API-Key
      x-aperture-secret:
        source: env
        name: API_KEY
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
";
    let temp_spec_path = PathBuf::from("/tmp/api_key_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_ok());

    // Verify both spec and cache files were created
    let spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join("api-key-api.yaml");
    let cache_path = PathBuf::from(TEST_CONFIG_DIR)
        .join(".cache")
        .join("api-key-api.bin");

    assert!(fs.exists(&spec_path));
    assert!(fs.exists(&cache_path));
}

#[test]
fn test_add_spec_with_valid_bearer_token_security() {
    let (manager, fs) = setup_manager();
    let spec_name = "bearer-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: Bearer Token API
  version: 1.0.0
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: BEARER_TOKEN
paths:
  /data:
    post:
      operationId: createData
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
      responses:
        '201':
          description: Created
";
    let temp_spec_path = PathBuf::from("/tmp/bearer_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_ok());
}

#[test]
fn test_add_spec_rejects_oauth2_security() {
    let (manager, fs) = setup_manager();
    let spec_name = "oauth2-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: OAuth2 API
  version: 1.0.0
components:
  securitySchemes:
    oauth2:
      type: oauth2
      flows:
        authorizationCode:
          authorizationUrl: https://example.com/auth
          tokenUrl: https://example.com/token
          scopes:
            read: Read access
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
";
    let temp_spec_path = PathBuf::from("/tmp/oauth2_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Internal {
        kind: ErrorKind::Validation,
        message: msg,
        ..
    }) = result
    {
        // The error message has changed due to our refactoring
        assert!(
            msg.contains("oauth2")
                || msg.contains("OAuth2")
                || msg.contains("unsupported authentication"),
            "Got validation message: {msg}"
        );
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_rejects_openid_connect_security() {
    let (manager, fs) = setup_manager();
    let spec_name = "openid-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: OpenID Connect API
  version: 1.0.0
components:
  securitySchemes:
    openId:
      type: openIdConnect
      openIdConnectUrl: https://example.com/.well-known/openid_configuration
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
";
    let temp_spec_path = PathBuf::from("/tmp/openid_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Internal {
        kind: ErrorKind::Validation,
        message: msg,
        ..
    }) = result
    {
        // The error message has changed due to our refactoring
        assert!(
            msg.contains("OpenID Connect")
                || msg.contains("openidconnect")
                || msg.contains("unsupported authentication"),
            "Got validation message: {msg}"
        );
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_rejects_unsupported_http_scheme() {
    let (manager, fs) = setup_manager();
    let spec_name = "negotiate-auth-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: Negotiate Auth API
  version: 1.0.0
components:
  securitySchemes:
    negotiateAuth:
      type: http
      scheme: negotiate
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
";
    let temp_spec_path = PathBuf::from("/tmp/negotiate_auth_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Internal {
        kind: ErrorKind::Validation,
        message: msg,
        ..
    }) = result
    {
        assert!(msg.contains("HTTP scheme 'negotiate'"));
        assert!(msg.contains("requires complex authentication flows"));
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_rejects_unsupported_request_body_content_type() {
    let (manager, fs) = setup_manager();
    let spec_name = "xml-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: XML API
  version: 1.0.0
paths:
  /data:
    post:
      operationId: createData
      requestBody:
        required: true
        content:
          application/xml:
            schema:
              type: string
      responses:
        '201':
          description: Created
";
    let temp_spec_path = PathBuf::from("/tmp/xml_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Internal {
        kind: ErrorKind::Validation,
        message: msg,
        ..
    }) = result
    {
        assert!(msg.contains("Unsupported request body content type 'application/xml'"));
        assert!(msg.contains("Only 'application/json' is supported"));
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_requires_json_content_type() {
    let (manager, fs) = setup_manager();
    let spec_name = "no-json-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: No JSON API
  version: 1.0.0
paths:
  /data:
    post:
      operationId: createData
      requestBody:
        required: true
        content:
          text/plain:
            schema:
              type: string
      responses:
        '201':
          description: Created
";
    let temp_spec_path = PathBuf::from("/tmp/no_json_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_err());
    if let Err(Error::Internal {
        kind: ErrorKind::Validation,
        message: msg,
        ..
    }) = result
    {
        assert!(msg.contains("Unsupported request body content type 'text/plain'"));
        assert!(msg.contains("Only 'application/json' is supported"));
    } else {
        panic!("Unexpected error type: {result:?}");
    }
}

#[test]
fn test_add_spec_caching_creates_correct_structure() {
    let (manager, fs) = setup_manager();
    let spec_name = "caching-test-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: Caching Test API
  version: 2.1.0
paths:
  /users:
    get:
      operationId: listUsers
      summary: List all users
      parameters:
        - name: limit
          in: query
          required: false
          schema:
            type: integer
      responses:
        '200':
          description: Success
  /users/{id}:
    get:
      operationId: getUser
      summary: Get user by ID
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
      responses:
        '200':
          description: Success
        '404':
          description: Not found
";
    let temp_spec_path = PathBuf::from("/tmp/caching_test_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_ok());

    // Verify cache file was created
    let cache_path = PathBuf::from(TEST_CONFIG_DIR)
        .join(".cache")
        .join("caching-test-api.bin");

    assert!(fs.exists(&cache_path));

    // Verify cache file contains serialized data (should be non-empty binary)
    let cache_content = fs.files.lock().unwrap().get(&cache_path).cloned();
    assert!(cache_content.is_some());
    let cache_data = cache_content.unwrap();
    assert!(!cache_data.is_empty());

    // Verify it's valid postcard by attempting to deserialize
    let cached_spec: Result<aperture_cli::cache::models::CachedSpec, _> =
        postcard::from_bytes(&cache_data);
    assert!(cached_spec.is_ok());

    let spec = cached_spec.unwrap();
    assert_eq!(spec.name, "caching-test-api");
    assert_eq!(spec.version, "2.1.0");
    assert_eq!(spec.commands.len(), 2);

    // Verify commands have tag names (default since no tags in spec)
    let mut command_tags: Vec<_> = spec.commands.iter().map(|c| c.name.clone()).collect();
    command_tags.sort();
    assert_eq!(command_tags, vec!["default", "default"]);

    // Verify operation IDs are preserved
    let mut operation_ids: Vec<_> = spec
        .commands
        .iter()
        .map(|c| c.operation_id.clone())
        .collect();
    operation_ids.sort();
    assert_eq!(operation_ids, vec!["getUser", "listUsers"]);
}

#[test]
fn test_add_spec_operation_id_fallback_to_method() {
    let (manager, fs) = setup_manager();
    let spec_name = "no-operation-id-api";
    let spec_content = r"
openapi: 3.0.0
info:
  title: No Operation ID API
  version: 1.0.0
paths:
  /data:
    get:
      summary: Get data without operationId
      responses:
        '200':
          description: Success
";
    let temp_spec_path = PathBuf::from("/tmp/no_operation_id_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);

    let result = manager.add_spec(&name(spec_name), &temp_spec_path, false, true);
    assert!(result.is_ok());

    // Verify cache was created with method name as command
    let cache_path = PathBuf::from(TEST_CONFIG_DIR)
        .join(".cache")
        .join("no-operation-id-api.bin");

    let cache_data = fs.files.lock().unwrap().get(&cache_path).cloned().unwrap();
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        postcard::from_bytes(&cache_data).unwrap();

    assert_eq!(cached_spec.commands.len(), 1);
    assert_eq!(cached_spec.commands[0].name, "default"); // No tags, so default
    assert_eq!(cached_spec.commands[0].operation_id, "GET_/data"); // Falls back to method_path
    assert_eq!(cached_spec.commands[0].method, "GET");
}

#[test]
fn test_set_url_base_override() {
    let (manager, fs) = setup_manager();
    let spec_name = "test-api";

    // First add a spec
    let spec_content = create_minimal_spec();
    let temp_spec_path = PathBuf::from("/tmp/test_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);
    manager
        .add_spec(&name(spec_name), &temp_spec_path, false, true)
        .unwrap();

    // Set base URL
    let result = manager.set_url(&name(spec_name), "https://custom.example.com", None);
    assert!(result.is_ok());

    // Verify config was saved
    let config_path = PathBuf::from(TEST_CONFIG_DIR).join("config.toml");
    assert!(fs.exists(&config_path));

    // Load and check config
    let config = manager.load_global_config().unwrap();
    assert!(config.api_configs.contains_key(spec_name));
    let api_config = &config.api_configs[spec_name];
    assert_eq!(
        api_config.base_url_override,
        Some("https://custom.example.com".to_string())
    );
}

#[test]
fn test_set_url_environment_specific() {
    let (manager, fs) = setup_manager();
    let spec_name = "test-api";

    // First add a spec
    let spec_content = create_minimal_spec();
    let temp_spec_path = PathBuf::from("/tmp/test_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);
    manager
        .add_spec(&name(spec_name), &temp_spec_path, false, true)
        .unwrap();

    // Set environment-specific URLs
    manager
        .set_url(
            &name(spec_name),
            "https://staging.example.com",
            Some("staging"),
        )
        .unwrap();
    manager
        .set_url(&name(spec_name), "https://prod.example.com", Some("prod"))
        .unwrap();

    // Load and check config
    let config = manager.load_global_config().unwrap();
    let api_config = &config.api_configs[spec_name];
    assert_eq!(
        api_config.environment_urls.get("staging"),
        Some(&"https://staging.example.com".to_string())
    );
    assert_eq!(
        api_config.environment_urls.get("prod"),
        Some(&"https://prod.example.com".to_string())
    );
}

#[test]
fn test_set_url_spec_not_found() {
    let (manager, _fs) = setup_manager();

    let result = manager.set_url(&name("nonexistent"), "https://example.com", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[test]
fn test_get_url_returns_config() {
    let (manager, fs) = setup_manager();
    let spec_name = "test-api";

    // First add a spec with base URL
    let spec_content = r"
openapi: 3.0.0
info:
  title: Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
paths:
  /test:
    get:
      responses:
        '200':
          description: Success
";
    let temp_spec_path = PathBuf::from("/tmp/test_api.yaml");
    fs.add_file(&temp_spec_path, spec_content);
    manager
        .add_spec(&name(spec_name), &temp_spec_path, false, true)
        .unwrap();

    // Set some URLs
    manager
        .set_url(&name(spec_name), "https://override.example.com", None)
        .unwrap();
    manager
        .set_url(&name(spec_name), "https://dev.example.com", Some("dev"))
        .unwrap();

    // Get URL config - this will fail to load cached spec but should still return config
    let result = manager.get_url(&name(spec_name));
    assert!(result.is_ok());
    let (base_override, env_urls, _resolved) = result.unwrap();

    assert_eq!(
        base_override,
        Some("https://override.example.com".to_string())
    );
    assert_eq!(
        env_urls.get("dev"),
        Some(&"https://dev.example.com".to_string())
    );
    // Note: resolved URL will be fallback since cached spec can't be loaded in test environment
}

#[test]
fn test_list_urls_shows_all_configs() {
    let (manager, fs) = setup_manager();

    // Add two specs
    let spec_content = create_minimal_spec();
    let temp_spec_path1 = PathBuf::from("/tmp/api1.yaml");
    let temp_spec_path2 = PathBuf::from("/tmp/api2.yaml");
    fs.add_file(&temp_spec_path1, spec_content);
    fs.add_file(&temp_spec_path2, spec_content);

    manager
        .add_spec(&name("api1"), &temp_spec_path1, false, true)
        .unwrap();
    manager
        .add_spec(&name("api2"), &temp_spec_path2, false, true)
        .unwrap();

    // Set URLs for both
    manager
        .set_url(&name("api1"), "https://api1.example.com", None)
        .unwrap();
    manager
        .set_url(&name("api2"), "https://api2.example.com", None)
        .unwrap();
    manager
        .set_url(&name("api2"), "https://api2-prod.example.com", Some("prod"))
        .unwrap();

    // List URLs
    let all_urls = manager.list_urls().unwrap();

    assert_eq!(all_urls.len(), 2);
    assert!(all_urls.contains_key("api1"));
    assert!(all_urls.contains_key("api2"));

    let (api1_base, api1_envs) = &all_urls["api1"];
    assert_eq!(api1_base, &Some("https://api1.example.com".to_string()));
    assert!(api1_envs.is_empty());

    let (api2_base, api2_envs) = &all_urls["api2"];
    assert_eq!(api2_base, &Some("https://api2.example.com".to_string()));
    assert_eq!(
        api2_envs.get("prod"),
        Some(&"https://api2-prod.example.com".to_string())
    );
}

// --- Remote Spec Support Tests (Feature 2.1) ---
// Tests are ignored until implementation is ready

#[test]
fn test_url_detection_http() {
    // Test that URLs starting with http:// are detected as URLs
    assert!(is_url("http://api.example.com/openapi.yaml"));
    assert!(is_url("http://localhost:8080/spec.yaml"));
}

#[test]
fn test_url_detection_https() {
    // Test that URLs starting with https:// are detected as URLs
    assert!(is_url("https://api.example.com/openapi.yaml"));
    assert!(is_url("https://petstore.swagger.io/v2/swagger.json"));
}

#[test]
fn test_url_detection_file_paths() {
    // Test that file paths are not detected as URLs
    assert!(!is_url("/path/to/spec.yaml"));
    assert!(!is_url("./relative/spec.yaml"));
    assert!(!is_url("../parent/spec.yaml"));
    assert!(!is_url("spec.yaml"));
    assert!(!is_url("C:\\Windows\\spec.yaml"));
}

#[tokio::test]
async fn test_remote_spec_fetching_success() {
    // Test successful remote spec fetching with valid OpenAPI
    let mock_server = wiremock::MockServer::start().await;
    let spec_content = r"
openapi: 3.0.0
info:
  title: Remote API
  version: 1.0.0
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
";

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/openapi.yaml"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(spec_content))
        .mount(&mock_server)
        .await;

    let (manager, _fs) = setup_manager();
    let spec_url = format!("{}/openapi.yaml", mock_server.uri());

    // Test that adding a remote spec works
    let result = manager
        .add_spec_from_url(&name("remote-api"), &spec_url, false, true)
        .await;
    assert!(result.is_ok());

    // Verify the spec was added to the list
    let specs = manager.list_specs().unwrap();
    assert!(specs.contains(&"remote-api".to_string()));
}

#[tokio::test]
async fn test_remote_spec_fetching_timeout() {
    // Test that HTTP requests timeout with configurable timeout (fast test)
    let mock_server = wiremock::MockServer::start().await;

    // Mock a response that takes longer than timeout (2s > 1s timeout)
    // We'll modify the timeout for this test to be shorter for faster testing
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/slow-spec.yaml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(2))
                .set_body_string(
                    "openapi: 3.0.0\ninfo:\n  title: Slow API\n  version: 1.0.0\npaths: {}",
                ),
        )
        .mount(&mock_server)
        .await;

    let (manager, _fs) = setup_manager();
    let spec_url = format!("{}/slow-spec.yaml", mock_server.uri());

    // Test that the request times out using a 1-second timeout instead of 30 seconds
    let result = manager
        .add_spec_from_url_with_timeout(
            &name("slow-api"),
            &spec_url,
            false,
            std::time::Duration::from_secs(1),
        )
        .await;
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind: ErrorKind::Network,
            message,
            ..
        }) => {
            assert!(message.contains("timed out"));
        }
        _ => panic!("Expected Network error with timeout, got: {result:?}"),
    }
}

#[tokio::test]
async fn test_remote_spec_fetching_size_limit() {
    // Test that responses larger than 10MB are rejected
    let mock_server = wiremock::MockServer::start().await;

    // Create a large response (>10MB)
    let large_content = "x".repeat(11 * 1024 * 1024); // 11MB

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/large-spec.yaml"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(large_content))
        .mount(&mock_server)
        .await;

    let (manager, _fs) = setup_manager();
    let spec_url = format!("{}/large-spec.yaml", mock_server.uri());

    // Test that large responses are rejected
    let result = manager
        .add_spec_from_url(&name("large-api"), &spec_url, false, true)
        .await;
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind: ErrorKind::Network,
            message,
            ..
        }) => {
            assert!(message.contains("too large"));
        }
        _ => panic!("Expected Network error for size limit, got: {result:?}"),
    }
}

#[tokio::test]
async fn test_remote_spec_fetching_invalid_url() {
    // Test error handling for invalid URLs
    let (manager, _fs) = setup_manager();

    // Test with a completely invalid URL that will fail to connect
    let result = manager
        .add_spec_from_url(
            &name("invalid-api"),
            "https://nonexistent-domain-12345.com/spec.yaml",
            false,
            true,
        )
        .await;
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind: ErrorKind::Network,
            message,
            ..
        }) => {
            assert!(message.contains("Failed to connect") || message.contains("Network error"));
        }
        _ => panic!("Expected Network error for invalid URL, got: {result:?}"),
    }
}

#[tokio::test]
async fn test_remote_spec_fetching_http_error() {
    // Test error handling for HTTP errors (404, 500, etc.)
    let mock_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/not-found.yaml"))
        .respond_with(wiremock::ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    let (manager, _fs) = setup_manager();
    let spec_url = format!("{}/not-found.yaml", mock_server.uri());

    // Test that HTTP errors are handled properly
    let result = manager
        .add_spec_from_url(&name("not-found-api"), &spec_url, false, true)
        .await;
    assert!(result.is_err());
    match result {
        Err(Error::Internal {
            kind: ErrorKind::HttpRequest,
            message,
            ..
        }) => {
            assert!(message.contains("404"));
        }
        _ => panic!("Expected RequestFailed error for HTTP 404, got: {result:?}"),
    }
}

#[tokio::test]
async fn test_remote_spec_fetching_invalid_yaml() {
    // Test error handling for invalid YAML content
    let mock_server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/invalid.yaml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_string("invalid: yaml: content: ["),
        )
        .mount(&mock_server)
        .await;

    let (manager, _fs) = setup_manager();
    let spec_url = format!("{}/invalid.yaml", mock_server.uri());

    // Test that invalid YAML is rejected
    let result = manager
        .add_spec_from_url(&name("invalid-yaml-api"), &spec_url, false, true)
        .await;
    assert!(result.is_err());
    // Should get a YAML parsing error
    assert!(matches!(result, Err(Error::Yaml(_))));
}

#[tokio::test]
async fn test_remote_spec_same_validation_as_local() {
    // Test that remote specs go through the same validation as local files
    let mock_server = wiremock::MockServer::start().await;

    // Spec with unsupported OAuth2 (should be rejected)
    let invalid_spec = r"
openapi: 3.0.0
info:
  title: OAuth2 API
  version: 1.0.0
components:
  securitySchemes:
    oauth2:
      type: oauth2
      flows:
        authorizationCode:
          authorizationUrl: https://example.com/auth
          tokenUrl: https://example.com/token
          scopes:
            read: Read access
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
";

    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/oauth2-spec.yaml"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(invalid_spec))
        .mount(&mock_server)
        .await;

    let (manager, _fs) = setup_manager();
    let spec_url = format!("{}/oauth2-spec.yaml", mock_server.uri());

    // Test that remote specs are validated the same as local files
    let result = manager
        .add_spec_from_url(&name("oauth2-api"), &spec_url, false, true)
        .await;
    assert!(result.is_err());
    if let Err(Error::Internal {
        kind: ErrorKind::Validation,
        message: msg,
        ..
    }) = result
    {
        // Check for any OAuth2-related validation error
        assert!(
            msg.contains("oauth2") || msg.contains("OAuth2"),
            "Got validation message: {msg}"
        );
    } else {
        panic!("Expected Validation error for OAuth2, got: {result:?}");
    }
}

// ============================================================================
// Settings Management Tests
// ============================================================================

#[test]
fn test_set_setting_timeout() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, _fs) = setup_manager();

    // Set timeout to 60 seconds
    let result = manager.set_setting(&SettingKey::DefaultTimeoutSecs, &SettingValue::U64(60));
    assert!(result.is_ok());

    // Verify the value was set
    let value = manager
        .get_setting(&SettingKey::DefaultTimeoutSecs)
        .unwrap();
    assert_eq!(value, SettingValue::U64(60));
}

#[test]
fn test_set_setting_json_errors() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, _fs) = setup_manager();

    // Set json_errors to true
    let result = manager.set_setting(
        &SettingKey::AgentDefaultsJsonErrors,
        &SettingValue::Bool(true),
    );
    assert!(result.is_ok());

    // Verify the value was set
    let value = manager
        .get_setting(&SettingKey::AgentDefaultsJsonErrors)
        .unwrap();
    assert_eq!(value, SettingValue::Bool(true));
}

#[test]
fn test_get_setting_default_timeout() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, _fs) = setup_manager();

    // Default timeout should be 30
    let value = manager
        .get_setting(&SettingKey::DefaultTimeoutSecs)
        .unwrap();
    assert_eq!(value, SettingValue::U64(30));
}

#[test]
fn test_get_setting_default_json_errors() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, _fs) = setup_manager();

    // Default json_errors should be false
    let value = manager
        .get_setting(&SettingKey::AgentDefaultsJsonErrors)
        .unwrap();
    assert_eq!(value, SettingValue::Bool(false));
}

#[test]
fn test_list_settings() {
    let (manager, _fs) = setup_manager();

    let settings = manager.list_settings().unwrap();

    // Should have 5 settings (timeout, json_errors, and 3 retry settings)
    assert_eq!(settings.len(), 5);

    // Check setting keys are present
    let keys: Vec<_> = settings.iter().map(|s| s.key.as_str()).collect();
    assert!(keys.contains(&"default_timeout_secs"));
    assert!(keys.contains(&"agent_defaults.json_errors"));
    assert!(keys.contains(&"retry_defaults.max_attempts"));
    assert!(keys.contains(&"retry_defaults.initial_delay_ms"));
    assert!(keys.contains(&"retry_defaults.max_delay_ms"));
}

#[test]
fn test_set_setting_preserves_existing_config() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, fs) = setup_manager();

    // First, add a spec to create some config
    let spec_content = r#"
openapi: "3.0.0"
info:
  title: Test API
  version: "1.0"
paths:
  /test:
    get:
      operationId: testOp
      responses:
        "200":
          description: OK
"#;
    let spec_path = manager.config_dir().join("specs").join("test-api.yaml");
    fs.add_file(&spec_path, spec_content);

    // Set a timeout value
    manager
        .set_setting(&SettingKey::DefaultTimeoutSecs, &SettingValue::U64(45))
        .unwrap();

    // Set json_errors
    manager
        .set_setting(
            &SettingKey::AgentDefaultsJsonErrors,
            &SettingValue::Bool(true),
        )
        .unwrap();

    // Verify both settings are preserved
    let timeout = manager
        .get_setting(&SettingKey::DefaultTimeoutSecs)
        .unwrap();
    assert_eq!(timeout, SettingValue::U64(45));

    let json_errors = manager
        .get_setting(&SettingKey::AgentDefaultsJsonErrors)
        .unwrap();
    assert_eq!(json_errors, SettingValue::Bool(true));
}

#[test]
fn test_list_settings_with_modified_values() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, _fs) = setup_manager();

    // Modify the timeout
    manager
        .set_setting(&SettingKey::DefaultTimeoutSecs, &SettingValue::U64(120))
        .unwrap();

    let settings = manager.list_settings().unwrap();

    // Find the timeout setting
    let timeout_setting = settings
        .iter()
        .find(|s| s.key == "default_timeout_secs")
        .unwrap();

    // Value should be the modified value
    assert_eq!(timeout_setting.value, "120");
    // Default should still show original default
    assert_eq!(timeout_setting.default, "30");
}

#[test]
fn test_set_setting_preserves_comments() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};

    let (manager, fs) = setup_manager();

    // Create a config file with comments
    let config_with_comments = r"# This is a comment about timeout
default_timeout_secs = 30

# Agent configuration section
[agent_defaults]
# Enable JSON error output for programmatic use
json_errors = false
";

    let config_path = manager.config_dir().join("config.toml");
    fs.add_file(&config_path, config_with_comments);

    // Modify a setting
    manager
        .set_setting(&SettingKey::DefaultTimeoutSecs, &SettingValue::U64(60))
        .unwrap();

    // Read the config file content back
    let content = fs.get_file_content(&config_path).unwrap();

    // Verify comments are preserved
    assert!(
        content.contains("# This is a comment about timeout"),
        "Comment about timeout should be preserved. Got:\n{content}"
    );
    assert!(
        content.contains("# Agent configuration section"),
        "Section comment should be preserved. Got:\n{content}"
    );
    assert!(
        content.contains("# Enable JSON error output"),
        "Inline comment should be preserved. Got:\n{content}"
    );

    // Verify the value was actually changed
    assert!(
        content.contains("60"),
        "New timeout value should be present. Got:\n{content}"
    );
}

// ── Command Mapping Management ──

/// Helper to set up a manager with a spec file present (needed for `ensure_command_mapping`)
fn setup_manager_with_spec(spec_name: &str) -> (ConfigManager<MockFileSystem>, MockFileSystem) {
    let (manager, fs) = setup_manager();
    let spec_path = PathBuf::from(TEST_CONFIG_DIR)
        .join("specs")
        .join(format!("{spec_name}.yaml"));
    fs.add_file(
        &spec_path,
        "openapi: 3.0.0\ninfo:\n  title: Test\n  version: 1.0.0\npaths: {}\n",
    );
    // Ensure a default config.toml exists
    let config_path = PathBuf::from(TEST_CONFIG_DIR).join("config.toml");
    if !fs.exists(&config_path) {
        fs.add_file(&config_path, "");
    }
    (manager, fs)
}

#[test]
fn test_set_group_mapping() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    manager
        .set_group_mapping(&api_name, "User Management", "users")
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap();
    let mapping = mapping.expect("mapping should exist");
    assert_eq!(
        mapping.groups.get("User Management"),
        Some(&"users".to_string())
    );
}

#[test]
fn test_set_group_mapping_nonexistent_spec_fails() {
    let (manager, _fs) = setup_manager();
    // config.toml needed for load_global_config to succeed
    let config_path = PathBuf::from(TEST_CONFIG_DIR).join("config.toml");
    _fs.add_file(&config_path, "");

    let api_name = name("nonexistent");
    let result = manager.set_group_mapping(&api_name, "tag", "renamed");
    assert!(result.is_err(), "Should fail for non-existent spec");
}

#[test]
fn test_remove_group_mapping() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    // Set then remove
    manager
        .set_group_mapping(&api_name, "User Management", "users")
        .unwrap();
    manager
        .remove_group_mapping(&api_name, "User Management")
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap();
    let mapping = mapping.expect("mapping should still exist as struct");
    assert!(
        !mapping.groups.contains_key("User Management"),
        "group mapping should have been removed"
    );
}

#[test]
fn test_remove_group_mapping_no_config_is_noop() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");
    // No mapping set — remove should succeed silently
    let result = manager.remove_group_mapping(&api_name, "anything");
    assert!(result.is_ok());
}

#[test]
fn test_set_operation_mapping_name_and_group() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    manager
        .set_operation_mapping(
            &api_name,
            "getUserById",
            Some("fetch"),
            Some("accounts"),
            None,
            None,
        )
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap().unwrap();
    let op = mapping.operations.get("getUserById").unwrap();
    assert_eq!(op.name, Some("fetch".to_string()));
    assert_eq!(op.group, Some("accounts".to_string()));
    assert!(op.aliases.is_empty());
    assert!(!op.hidden);
}

#[test]
fn test_set_operation_mapping_alias_deduplication() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    // Add alias twice — should only appear once
    manager
        .set_operation_mapping(&api_name, "getUser", None, None, Some("fetch"), None)
        .unwrap();
    manager
        .set_operation_mapping(&api_name, "getUser", None, None, Some("fetch"), None)
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap().unwrap();
    let op = mapping.operations.get("getUser").unwrap();
    assert_eq!(op.aliases, vec!["fetch".to_string()]);
}

#[test]
fn test_set_operation_mapping_hidden() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    manager
        .set_operation_mapping(&api_name, "deleteUser", None, None, None, Some(true))
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap().unwrap();
    let op = mapping.operations.get("deleteUser").unwrap();
    assert!(op.hidden);

    // Unhide
    manager
        .set_operation_mapping(&api_name, "deleteUser", None, None, None, Some(false))
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap().unwrap();
    let op = mapping.operations.get("deleteUser").unwrap();
    assert!(!op.hidden);
}

#[test]
fn test_remove_operation_mapping() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    manager
        .set_operation_mapping(
            &api_name,
            "getUserById",
            Some("fetch"),
            None,
            Some("get"),
            Some(true),
        )
        .unwrap();
    manager
        .remove_operation_mapping(&api_name, "getUserById")
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap().unwrap();
    assert!(
        !mapping.operations.contains_key("getUserById"),
        "operation mapping should have been removed"
    );
}

#[test]
fn test_get_command_mapping_no_config_returns_none() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    let mapping = manager.get_command_mapping(&api_name).unwrap();
    assert!(mapping.is_none(), "should be None when no mapping is set");
}

#[test]
fn test_set_operation_mapping_incremental_updates() {
    let (manager, _fs) = setup_manager_with_spec("myapi");
    let api_name = name("myapi");

    // First call: set name
    manager
        .set_operation_mapping(&api_name, "getUser", Some("fetch"), None, None, None)
        .unwrap();

    // Second call: add alias (name should be preserved)
    manager
        .set_operation_mapping(&api_name, "getUser", None, None, Some("get"), None)
        .unwrap();

    // Third call: set hidden (name and alias should be preserved)
    manager
        .set_operation_mapping(&api_name, "getUser", None, None, None, Some(true))
        .unwrap();

    let mapping = manager.get_command_mapping(&api_name).unwrap().unwrap();
    let op = mapping.operations.get("getUser").unwrap();
    assert_eq!(op.name, Some("fetch".to_string()));
    assert_eq!(op.aliases, vec!["get".to_string()]);
    assert!(op.hidden);
}
