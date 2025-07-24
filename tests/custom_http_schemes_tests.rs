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

    fn exists(&self, path: &Path) -> bool {
        let files = self.files.lock().unwrap();
        let dirs = self.dirs.lock().unwrap();
        files.contains_key(path) || dirs.contains_key(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        let mut files = self.files.lock().unwrap();
        if files.remove(path).is_some() {
            Ok(())
        } else {
            Err(io::Error::new(ErrorKind::NotFound, "File not found"))
        }
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let files = self.files.lock().unwrap();
        let entries: Vec<PathBuf> = files
            .keys()
            .filter(|p| p.parent() == Some(path))
            .cloned()
            .collect();
        Ok(entries)
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.lock().unwrap().contains_key(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.dirs.lock().unwrap().remove(path);
        Ok(())
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.dirs.lock().unwrap().contains_key(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

fn setup_manager() -> (ConfigManager<MockFileSystem>, MockFileSystem) {
    let fs = MockFileSystem::new();
    let config_dir = PathBuf::from("/test/config");
    fs.add_dir(&config_dir);
    let manager = ConfigManager::with_fs(fs.clone(), config_dir);
    (manager, fs)
}

fn setup_dir(fs: &MockFileSystem) -> PathBuf {
    let dir = PathBuf::from("/test/specs");
    fs.add_dir(&dir);
    dir
}

#[test]
fn test_add_spec_with_custom_http_scheme_token() {
    let (manager, fs) = setup_manager();

    // Create a spec with Token HTTP scheme (common alternative to Bearer)
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Token Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    tokenAuth:
      type: http
      scheme: Token
      x-aperture-secret:
        source: env
        name: API_TOKEN
paths:
  /users:
    get:
      operationId: getUsers
      security:
        - tokenAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = setup_dir(&fs).join("token-api.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed - Token scheme is now supported
    let result = manager.add_spec("token-api", &spec_path, false, false);
    assert!(result.is_ok(), "Token HTTP scheme should be supported");

    // Verify the spec was added
    let specs = manager.list_specs().expect("Failed to list specs");
    assert!(specs.contains(&"token-api".to_string()));
}

#[test]
fn test_add_spec_with_custom_http_scheme_apikey() {
    let (manager, fs) = setup_manager();

    // Create a spec with ApiKey HTTP scheme (note: different from apiKey type)
    let spec_content = r#"
openapi: 3.0.0
info:
  title: ApiKey Scheme API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    customAuth:
      type: http
      scheme: ApiKey
      x-aperture-secret:
        source: env
        name: CUSTOM_API_KEY
paths:
  /data:
    get:
      operationId: getData
      security:
        - customAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = setup_dir(&fs).join("apikey-scheme.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed - ApiKey HTTP scheme is now supported
    let result = manager.add_spec("apikey-scheme", &spec_path, false, false);
    assert!(result.is_ok(), "ApiKey HTTP scheme should be supported");
}

#[test]
fn test_add_spec_with_dsn_scheme() {
    let (manager, fs) = setup_manager();

    // Create a spec with DSN HTTP scheme (Sentry-style)
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Sentry-style API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    dsnAuth:
      type: http
      scheme: DSN
      x-aperture-secret:
        source: env
        name: SENTRY_DSN
paths:
  /events:
    post:
      operationId: sendEvent
      security:
        - dsnAuth: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = setup_dir(&fs).join("dsn-api.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed - DSN scheme is now supported
    let result = manager.add_spec("dsn-api", &spec_path, false, false);
    assert!(result.is_ok(), "DSN HTTP scheme should be supported");
}

#[test]
fn test_add_spec_with_proprietary_http_scheme() {
    let (manager, fs) = setup_manager();

    // Create a spec with a completely custom/proprietary HTTP scheme
    let spec_content = r#"
openapi: 3.0.0
info:
  title: Proprietary Auth API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    customScheme:
      type: http
      scheme: X-CompanyAuth-V2
      x-aperture-secret:
        source: env
        name: COMPANY_AUTH_TOKEN
paths:
  /protected:
    get:
      operationId: getProtected
      security:
        - customScheme: []
      responses:
        '200':
          description: Success
"#;

    let spec_path = setup_dir(&fs).join("proprietary-api.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should succeed - any custom scheme name is now supported
    let result = manager.add_spec("proprietary-api", &spec_path, false, false);
    assert!(
        result.is_ok(),
        "Custom proprietary HTTP schemes should be supported"
    );
}

#[test]
fn test_reject_oauth_http_scheme() {
    let (manager, fs) = setup_manager();

    // Create a spec with 'oauth' as HTTP scheme (should be rejected)
    let spec_content = r#"
openapi: 3.0.0
info:
  title: OAuth HTTP Scheme API
  version: 1.0.0
servers:
  - url: https://api.example.com
components:
  securitySchemes:
    oauthScheme:
      type: http
      scheme: oauth
paths:
  /users:
    get:
      operationId: getUsers
      responses:
        '200':
          description: Success
"#;

    let spec_path = setup_dir(&fs).join("oauth-http.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should fail - 'oauth' as HTTP scheme suggests complex flows
    let result = manager.add_spec("oauth-http", &spec_path, false, true); // Use strict mode
    assert!(result.is_err());
    if let Err(Error::Validation(msg)) = result {
        assert!(
            msg.contains("requires complex authentication flows"),
            "Expected complex auth flow error, got: {}",
            msg
        );
    } else {
        panic!("Expected Validation error for oauth HTTP scheme");
    }
}

#[test]
fn test_reject_negotiate_http_scheme() {
    let (manager, fs) = setup_manager();

    // Create a spec with 'negotiate' HTTP scheme (Kerberos/NTLM)
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
      scheme: Negotiate
paths:
  /secure:
    get:
      operationId: getSecure
      responses:
        '200':
          description: Success
"#;

    let spec_path = setup_dir(&fs).join("negotiate-api.yaml");
    fs.write_all(&spec_path, spec_content.as_bytes())
        .expect("Failed to write spec");

    // Should fail - Negotiate requires complex Kerberos/NTLM flows
    let result = manager.add_spec("negotiate-api", &spec_path, false, true); // Use strict mode
    assert!(result.is_err());
    if let Err(Error::Validation(msg)) = result {
        assert!(
            msg.contains("requires complex authentication flows"),
            "Expected complex auth flow error, got: {}",
            msg
        );
    } else {
        panic!("Expected Validation error for Negotiate scheme");
    }
}
