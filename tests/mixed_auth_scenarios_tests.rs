use aperture_cli::config::manager::ConfigManager;
use aperture_cli::error::Error;
use aperture_cli::fs::FileSystem;
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// Mock FileSystem implementation for testing
#[derive(Clone)]
pub struct MockFileSystem {
    files: Arc<Mutex<HashMap<PathBuf, Vec<u8>>>>,
    dirs: Arc<Mutex<HashMap<PathBuf, bool>>>,
}

impl MockFileSystem {
    pub fn new() -> Self {
        Self {
            files: Arc::new(Mutex::new(HashMap::new())),
            dirs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl FileSystem for MockFileSystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        let files = self.files.lock().unwrap();
        if let Some(content) = files.get(path) {
            String::from_utf8(content.clone())
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Invalid UTF-8 in file"))
        } else {
            Err(io::Error::new(ErrorKind::NotFound, "File not found"))
        }
    }

    fn write_all(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        files.insert(path.to_path_buf(), contents.to_vec());
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
            || self.dirs.lock().unwrap().contains_key(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut dirs = self.dirs.lock().unwrap();
        dirs.insert(path.to_path_buf(), true);
        // Also create parent directories
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                dirs.insert(parent.to_path_buf(), true);
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if files.remove(path).is_some() {
            Ok(())
        } else {
            Err(io::Error::new(ErrorKind::NotFound, "File not found"))
        }
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        let mut dirs = self.dirs.lock().unwrap();
        let mut files = self.files.lock().unwrap();

        // Remove the directory
        dirs.remove(path);

        // Remove all files in the directory
        let path_str = path.to_string_lossy();
        files.retain(|k, _| !k.to_string_lossy().starts_with(&*path_str));

        Ok(())
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
        let path_str = path.to_string_lossy();
        let entries: Vec<PathBuf> = files
            .keys()
            .filter_map(|k| {
                let k_str = k.to_string_lossy();
                if k_str.starts_with(&*path_str) && k_str.len() > path_str.len() {
                    let relative = &k_str[path_str.len()..];
                    let relative = relative.trim_start_matches('/');
                    if !relative.is_empty() && !relative.contains('/') {
                        return Some(PathBuf::from(relative));
                    }
                }
                None
            })
            .collect();
        Ok(entries)
    }
}

impl MockFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let files = self.files.lock().unwrap();
        files
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "File not found"))
    }
}

fn setup_manager() -> (ConfigManager<MockFileSystem>, MockFileSystem) {
    let fs = MockFileSystem::new();
    let config_dir = PathBuf::from("/tmp/aperture_test");

    // Create necessary directories
    fs.create_dir_all(&config_dir.join("specs"))
        .expect("Failed to create specs dir");
    fs.create_dir_all(&config_dir.join(".cache"))
        .expect("Failed to create cache dir");

    let manager = ConfigManager::with_fs(fs.clone(), config_dir);
    (manager, fs)
}

#[test]
fn test_spec_with_mixed_auth_non_strict() {
    let (manager, fs) = setup_manager();

    // Create a spec with both supported and unsupported auth schemes
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Mixed Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
    oauth2Auth:
      type: oauth2
      flows:
        authorizationCode:
          authorizationUrl: https://example.com/auth
          tokenUrl: https://example.com/token
          scopes:
            read: Read access
    apiKey:
      type: apiKey
      name: X-API-Key
      in: header
      x-aperture-secret:
        source: env
        name: API_KEY
paths:
  /users:
    get:
      operationId: getUsers
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
  /admin:
    get:
      operationId: getAdmin
      security:
        - oauth2Auth: [read]
      responses:
        '200':
          description: Success
  /public:
    get:
      operationId: getPublic
      responses:
        '200':
          description: Success
  /mixed:
    get:
      operationId: getMixed
      security:
        - bearerAuth: []
        - oauth2Auth: [read]
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/mixed-auth.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed in non-strict mode with warnings
    let result = manager.add_spec("mixed-auth", &spec_path, false, false);
    assert!(
        result.is_ok(),
        "Expected success in non-strict mode, got: {:?}",
        result
    );

    // Check that the spec was cached
    let cache_path = PathBuf::from("/tmp/aperture_test/.cache/mixed-auth.bin");
    assert!(fs.exists(&cache_path), "Cache file should exist");

    // Load the cached spec to verify operations
    let cached_content = fs.read(&cache_path).expect("Failed to read cache");
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).expect("Failed to deserialize");

    // Should have 3 operations (getUsers, getPublic, getMixed) - getAdmin should be skipped
    assert_eq!(
        cached_spec.commands.len(),
        3,
        "Should have 3 available operations"
    );

    let op_names: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|c| c.operation_id.as_str())
        .collect();
    assert!(op_names.contains(&"getUsers"), "Should include getUsers");
    assert!(op_names.contains(&"getPublic"), "Should include getPublic");
    assert!(
        op_names.contains(&"getMixed"),
        "Should include getMixed (has alternative auth)"
    );
    assert!(
        !op_names.contains(&"getAdmin"),
        "Should not include getAdmin (only OAuth2)"
    );

    // Check skipped endpoints
    assert_eq!(
        cached_spec.skipped_endpoints.len(),
        1,
        "Should have 1 skipped endpoint"
    );
    let skipped = &cached_spec.skipped_endpoints[0];
    assert_eq!(skipped.path, "/admin");
    assert_eq!(skipped.method, "GET");
    assert!(skipped.reason.contains("unsupported authentication"));
}

#[test]
fn test_spec_with_mixed_auth_strict() {
    let (manager, fs) = setup_manager();

    // Same spec as above
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Mixed Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
    oauth2Auth:
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
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/mixed-auth-strict.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should fail in strict mode
    let result = manager.add_spec("mixed-auth-strict", &spec_path, false, true);
    assert!(result.is_err(), "Expected failure in strict mode");

    if let Err(Error::Validation(msg)) = result {
        assert!(
            msg.contains("OAuth2") || msg.contains("unsupported authentication"),
            "Expected OAuth2 error, got: {}",
            msg
        );
    } else {
        panic!("Expected Validation error, got: {:?}", result);
    }
}

#[test]
fn test_operation_with_empty_security_array() {
    let (manager, fs) = setup_manager();

    // Create a spec with operations having empty security arrays
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Empty Security API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    oauth2Auth:
      type: oauth2
      flows:
        authorizationCode:
          authorizationUrl: https://example.com/auth
          tokenUrl: https://example.com/token
          scopes:
            read: Read access
security:
  - oauth2Auth: [read]
paths:
  /public:
    get:
      operationId: getPublic
      security: []  # Empty array means no auth required
      responses:
        '200':
          description: Success
  /private:
    get:
      operationId: getPrivate
      # Uses global security (OAuth2)
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/empty-security.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed in non-strict mode
    let result = manager.add_spec("empty-security", &spec_path, false, false);
    assert!(
        result.is_ok(),
        "Expected success in non-strict mode, got: {:?}",
        result
    );

    // Check cached spec
    let cache_path = PathBuf::from("/tmp/aperture_test/.cache/empty-security.bin");
    assert!(fs.exists(&cache_path), "Cache file should exist");

    let cached_content = fs.read(&cache_path).expect("Failed to read cache");
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).expect("Failed to deserialize");

    // Should have only getPublic operation (getPrivate uses global OAuth2)
    assert_eq!(
        cached_spec.commands.len(),
        1,
        "Should have 1 available operation"
    );
    assert_eq!(
        cached_spec.commands[0].operation_id, "getPublic",
        "Should include getPublic"
    );

    // Should have one skipped endpoint
    assert_eq!(
        cached_spec.skipped_endpoints.len(),
        1,
        "Should have 1 skipped endpoint"
    );
    assert_eq!(cached_spec.skipped_endpoints[0].path, "/private");
}

#[test]
fn test_invalid_env_var_characters_in_auth() {
    let (manager, fs) = setup_manager();

    // Create a spec with basic auth for testing environment variables
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Env Var Test API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: BEARER_TOKEN_123  # Valid env var name
    invalidAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: 123_INVALID  # Starts with digit - invalid
paths:
  /users:
    get:
      operationId: getUsers
      security:
        - bearerAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/env-var-test.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should fail due to invalid environment variable name
    let result = manager.add_spec("env-var-test", &spec_path, false, false);
    assert!(
        result.is_err(),
        "Expected failure due to invalid env var name"
    );

    if let Err(Error::Validation(msg)) = result {
        assert!(
            msg.contains("Invalid environment variable name"),
            "Expected invalid env var error, got: {}",
            msg
        );
    } else {
        panic!("Expected Validation error, got: {:?}", result);
    }
}

#[test]
fn test_global_security_with_unsupported_auth() {
    let (manager, fs) = setup_manager();

    // Create a spec with global security using unsupported auth
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Global Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
security:
  - oauth2Auth: [read]
components:
  securitySchemes:
    oauth2Auth:
      type: oauth2
      flows:
        clientCredentials:
          tokenUrl: https://example.com/token
          scopes:
            read: Read access
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
  /public:
    get:
      operationId: getPublic
      security: []  # No security requirement
      responses:
        '200':
          description: Success
  /admin:
    get:
      operationId: getAdmin
      security:
        - bearerAuth: []  # Override global with supported auth
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/global-auth.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed in non-strict mode
    let result = manager.add_spec("global-auth", &spec_path, false, false);
    assert!(
        result.is_ok(),
        "Expected success in non-strict mode, got: {:?}",
        result
    );

    // Load the cached spec
    let cache_path = PathBuf::from("/tmp/aperture_test/.cache/global-auth.bin");
    let cached_content = fs.read(&cache_path).expect("Failed to read cache");
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).expect("Failed to deserialize");

    // Should have 2 operations (getPublic with no auth, getAdmin with bearer override)
    assert_eq!(
        cached_spec.commands.len(),
        2,
        "Should have 2 available operations"
    );

    let op_names: Vec<&str> = cached_spec
        .commands
        .iter()
        .map(|c| c.operation_id.as_str())
        .collect();
    assert!(
        op_names.contains(&"getPublic"),
        "Should include getPublic (no auth)"
    );
    assert!(
        op_names.contains(&"getAdmin"),
        "Should include getAdmin (bearer override)"
    );
    assert!(
        !op_names.contains(&"getUsers"),
        "Should not include getUsers (inherits global OAuth2)"
    );

    // Check skipped endpoints
    assert_eq!(
        cached_spec.skipped_endpoints.len(),
        1,
        "Should have 1 skipped endpoint"
    );
    let skipped = &cached_spec.skipped_endpoints[0];
    assert_eq!(skipped.path, "/users");
    assert!(skipped.reason.contains("unsupported authentication"));
}

#[test]
fn test_negotiate_scheme_in_http() {
    let (manager, fs) = setup_manager();

    // Create a spec with negotiate HTTP scheme alongside bearer
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Negotiate Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    negotiateAuth:
      type: http
      scheme: negotiate
    bearerAuth:
      type: http
      scheme: bearer
      x-aperture-secret:
        source: env
        name: API_TOKEN
paths:
  /krb:
    get:
      operationId: getKrb
      security:
        - negotiateAuth: []
      responses:
        '200':
          description: Success
  /dual:
    get:
      operationId: getDual
      security:
        - negotiateAuth: []
        - bearerAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/negotiate-auth.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed in non-strict mode
    let result = manager.add_spec("negotiate-auth", &spec_path, false, false);
    assert!(
        result.is_ok(),
        "Expected success in non-strict mode, got: {:?}",
        result
    );

    // Load the cached spec
    let cache_path = PathBuf::from("/tmp/aperture_test/.cache/negotiate-auth.bin");
    let cached_content = fs.read(&cache_path).expect("Failed to read cache");
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).expect("Failed to deserialize");

    // Should have 1 operation (getDual has bearer alternative)
    assert_eq!(
        cached_spec.commands.len(),
        1,
        "Should have 1 available operation"
    );
    assert_eq!(cached_spec.commands[0].operation_id, "getDual");

    // Check skipped endpoints
    assert_eq!(
        cached_spec.skipped_endpoints.len(),
        1,
        "Should have 1 skipped endpoint"
    );
    let skipped = &cached_spec.skipped_endpoints[0];
    assert_eq!(skipped.path, "/krb");
    assert!(skipped.reason.contains("unsupported authentication"));
    assert!(skipped.reason.contains("negotiate"));
}

#[test]
fn test_openid_connect_scheme() {
    let (manager, fs) = setup_manager();

    // Create a spec with OpenID Connect
    let spec_content = r#"
openapi: 3.0.0
info:
  title: OIDC API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    oidc:
      type: openIdConnect
      openIdConnectUrl: https://example.com/.well-known/openid-configuration
paths:
  /profile:
    get:
      operationId: getProfile
      security:
        - oidc: [profile, email]
      responses:
        '200':
          description: Success
"#;

    let spec_path = PathBuf::from("/tmp/oidc-auth.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed in non-strict mode but skip all endpoints
    let result = manager.add_spec("oidc-auth", &spec_path, false, false);
    assert!(
        result.is_ok(),
        "Expected success in non-strict mode, got: {:?}",
        result
    );

    // Load the cached spec
    let cache_path = PathBuf::from("/tmp/aperture_test/.cache/oidc-auth.bin");
    let cached_content = fs.read(&cache_path).expect("Failed to read cache");
    let cached_spec: aperture_cli::cache::models::CachedSpec =
        bincode::deserialize(&cached_content).expect("Failed to deserialize");

    // Should have no operations
    assert_eq!(
        cached_spec.commands.len(),
        0,
        "Should have no available operations"
    );

    // All endpoints should be skipped
    assert_eq!(
        cached_spec.skipped_endpoints.len(),
        1,
        "Should have 1 skipped endpoint"
    );
    let skipped = &cached_spec.skipped_endpoints[0];
    assert!(skipped.reason.contains("unsupported authentication"));
    assert!(skipped.reason.contains("oidc"));
}
