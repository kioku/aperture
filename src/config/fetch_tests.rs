//! Local synthetic HTTPS transport tests; the fixture root is trusted only by these clients.
use super::*;
use crate::config::fetch_auth::FetchMethod;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

const CERT: &[u8] = include_bytes!("../../tests/fixtures/fetch-auth/cert.pem");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/fetch-auth/key.pem");
const SPEC: &str = "openapi: 3.0.3\ninfo:\n  title: Protected\n  version: '1'\npaths: {}\n";

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn new(response: impl Fn(&str) -> String + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        listener.set_nonblocking(true).unwrap();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from_pem_slice(CERT).unwrap()],
                PrivateKeyDer::from_pem_slice(KEY).unwrap(),
            )
            .unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_requests = requests.clone();
        let thread_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            serve(
                &listener,
                &Arc::new(config),
                &thread_requests,
                &thread_stop,
                response,
            );
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }

    fn builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .no_proxy()
            // A synthetic fixture must not depend on OS/keychain trust policy.
            // WebPKI still verifies the chain, validity, signatures and hostname.
            .tls_certs_only([reqwest::Certificate::from_pem(CERT).unwrap()])
            // The listener is IPv4-only; avoid platform-specific localhost resolution.
            .resolve("localhost", "127.0.0.1:0".parse().unwrap())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

fn serve(
    listener: &TcpListener,
    config: &Arc<rustls::ServerConfig>,
    requests: &Arc<Mutex<Vec<String>>>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
    response: impl Fn(&str) -> String,
) {
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let Ok((socket, _)) = listener.accept() else {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        };
        prepare_socket(&socket);
        let mut stream = rustls::StreamOwned::new(
            rustls::ServerConnection::new(config.clone()).unwrap(),
            socket,
        );
        let Some(request) = read_request(&mut stream) else {
            continue;
        };
        requests.lock().unwrap().push(request.clone());
        let _ = stream.write_all(response(&request).as_bytes());
        let _ = stream.flush();
    }
}

// Accepted sockets can inherit the polling listener's nonblocking mode on BSD/Windows.
// rustls and read_exact below require blocking I/O, bounded in both directions.
fn prepare_socket(socket: &TcpStream) {
    socket.set_nonblocking(false).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();
}

#[test]
fn accepted_socket_resets_inherited_nonblocking_mode() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut socket, _) = listener.accept().unwrap();
    // Simulate inheritance even on Linux, where accepted sockets normally block.
    socket.set_nonblocking(true).unwrap();
    let mut byte = [0];
    assert_eq!(
        socket.read_exact(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    prepare_socket(&socket);
    let sender = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        client.write_all(b"x").unwrap();
    });
    let result = socket.read_exact(&mut byte);
    sender.join().unwrap();
    result.unwrap();
    assert_eq!(byte, *b"x");
}

fn read_request(stream: &mut impl Read) -> Option<String> {
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).ok()?;
        request.push(byte[0]);
    }
    String::from_utf8(request).ok()
}

fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

async fn download(server: &Server, path: &str, auth: &FetchAuth) -> Result<String, Error> {
    fetch_spec_with_builder(
        &format!("{}{path}", server.url),
        std::time::Duration::from_secs(3),
        Some(auth),
        Server::builder(),
    )
    .await
}

#[tokio::test]
async fn https_methods_and_rotated_values() {
    let server = Server::new(|_| ok(SPEC));
    check_method(
        &server,
        FetchMethod::Basic,
        None,
        "user:pass:colon",
        "authorization: Basic dXNlcjpwYXNzOmNvbG9u",
    )
    .await;
    check_method(
        &server,
        FetchMethod::Bearer,
        None,
        "synthetic-token",
        "authorization: Bearer synthetic-token",
    )
    .await;
    check_method(
        &server,
        FetchMethod::Header,
        Some("X-Spec-Key"),
        "synthetic-key",
        "x-spec-key: synthetic-key",
    )
    .await;
}

async fn check_method(
    server: &Server,
    method: FetchMethod,
    header: Option<&str>,
    value: &str,
    expected: &str,
) {
    let env = format!("APERTURE_FETCH_TEST_{method:?}");
    std::env::set_var(&env, value);
    let auth = FetchAuth::new(method, &env, header, &server.url).unwrap();
    assert_eq!(download(server, "/spec", &auth).await.unwrap(), SPEC);
    assert!(server
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains(expected));
    std::env::set_var(&env, "rotated:synthetic");
    if method != FetchMethod::Bearer {
        assert!(download(server, "/spec", &auth).await.is_ok());
    }
    std::env::remove_var(&env);
}

#[tokio::test]
async fn same_origin_redirect_and_bounded_loop() {
    let server = Server::new(|request| {
        if request.starts_with("GET /spec ") {
            return ok(SPEC);
        }
        let location = if request.starts_with("GET /loop ") {
            "/loop"
        } else {
            "/spec"
        };
        format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    });
    std::env::set_var("APERTURE_FETCH_REDIRECT", "synthetic");
    let auth = FetchAuth::new(
        FetchMethod::Header,
        "APERTURE_FETCH_REDIRECT",
        Some("X-Key"),
        &server.url,
    )
    .unwrap();
    assert_eq!(download(&server, "/start", &auth).await.unwrap(), SPEC);
    assert!(download(&server, "/loop", &auth).await.is_err());
    std::env::remove_var("APERTURE_FETCH_REDIRECT");
}

#[tokio::test]
async fn unsafe_redirects_receive_no_credentials() {
    let destination = Server::new(|_| ok(SPEC));
    for target in [
        format!("{}/spec", destination.url),
        "http://localhost:1/spec".into(),
        "https://user:pass@localhost/spec".into(),
    ] {
        reject_redirect(&target).await;
    }
    assert!(destination.requests.lock().unwrap().is_empty());
}

async fn reject_redirect(target: &str) {
    let target = target.to_owned();
    let server = Server::new(move |_| {
        format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    });
    std::env::set_var("APERTURE_FETCH_UNSAFE", "synthetic-private");
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "APERTURE_FETCH_UNSAFE",
        None,
        &server.url,
    )
    .unwrap();
    let error = download(&server, "/spec", &auth)
        .await
        .unwrap_err()
        .to_string();
    assert!(!error.contains("synthetic-private"));
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    std::env::remove_var("APERTURE_FETCH_UNSAFE");
}

#[tokio::test]
async fn errors_do_not_echo_authentication_response_bodies() {
    for status in [401, 403] {
        let server = Server::new(move |_| {
            format!("HTTP/1.1 {status} Denied\r\nContent-Length: 17\r\nConnection: close\r\n\r\nsynthetic-private")
        });
        std::env::set_var("APERTURE_FETCH_DENIED", "synthetic-private");
        let auth = FetchAuth::new(
            FetchMethod::Bearer,
            "APERTURE_FETCH_DENIED",
            None,
            &server.url,
        )
        .unwrap();
        let error = download(&server, "/spec", &auth)
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("synthetic-private"));
        std::env::remove_var("APERTURE_FETCH_DENIED");
    }
}

#[tokio::test]
async fn missing_empty_credentials_make_no_request() {
    let server = Server::new(|_| ok(SPEC));
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "APERTURE_FETCH_MISSING",
        None,
        &server.url,
    )
    .unwrap();
    std::env::remove_var("APERTURE_FETCH_MISSING");
    assert!(download(&server, "/spec", &auth).await.is_err());
    std::env::set_var("APERTURE_FETCH_MISSING", "");
    assert!(download(&server, "/spec", &auth).await.is_err());
    assert!(server.requests.lock().unwrap().is_empty());
    std::env::remove_var("APERTURE_FETCH_MISSING");
}

#[tokio::test]
async fn oversized_authenticated_response_is_rejected() {
    let server = Server::new(|_| {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_RESPONSE_SIZE + 1
        )
    });
    std::env::set_var("APERTURE_FETCH_SIZE", "synthetic");
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "APERTURE_FETCH_SIZE",
        None,
        &server.url,
    )
    .unwrap();
    assert!(download(&server, "/spec", &auth).await.is_err());
    std::env::remove_var("APERTURE_FETCH_SIZE");
}

async fn register(
    manager: &ConfigManager<OsFileSystem>,
    server: &Server,
    args: &FetchAuthArgs,
) -> Result<(), Error> {
    register_with_strict(manager, server, args, false).await
}

async fn register_with_strict(
    manager: &ConfigManager<OsFileSystem>,
    server: &Server,
    args: &FetchAuthArgs,
    strict: bool,
) -> Result<(), Error> {
    let name = ApiContextName::new("protected").unwrap();
    let config = manager.load_global_config()?;
    let saved = config
        .api_configs
        .get("protected")
        .and_then(|api| api.fetch_auth.as_ref());
    let auth = args.select(&server.url, saved)?;
    let (content, credentials) = fetch_spec_with_credentials(
        &server.url,
        std::time::Duration::from_secs(3),
        auth.as_ref(),
        Server::builder(),
    )
    .await?;
    manager.register_guarded_spec(&name, &content, strict, auth, credentials.as_ref())
}

#[tokio::test]
async fn registration_rotation_offline_reinit_and_source_replacement() {
    let server = Server::new(|_| ok(SPEC));
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let name = ApiContextName::new("protected").unwrap();
    std::env::set_var("APERTURE_FETCH_REGISTER", "synthetic-token-first");
    let args = FetchAuthArgs {
        fetch_auth: Some(FetchMethod::Bearer),
        fetch_auth_env: Some("APERTURE_FETCH_REGISTER".into()),
        fetch_header_name: None,
    };
    register(&manager, &server, &args).await.unwrap();
    manager
        .set_secret(&name, "operation-key", "OPERATION_KEY")
        .unwrap();
    std::env::set_var("APERTURE_FETCH_REGISTER", "synthetic-token-second");
    register(&manager, &server, &FetchAuthArgs::default())
        .await
        .unwrap();
    assert!(server
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains("synthetic-token-second"));
    assert_reference_only(&manager, dir.path());
    std::env::remove_var("APERTURE_FETCH_REGISTER");
    let path = dir.path().join("specs/protected.yaml");
    manager.reinit_local_spec(&name, &path, false).unwrap();
    assert!(
        manager.load_global_config().unwrap().api_configs["protected"]
            .fetch_auth
            .is_some()
    );
    manager.add_spec(&name, &path, true, false).unwrap();
    let api = manager
        .load_global_config()
        .unwrap()
        .api_configs
        .remove("protected")
        .unwrap();
    assert!(api.fetch_auth.is_none());
    assert!(api.secrets.contains_key("operation-key"));
}

fn assert_reference_only(manager: &ConfigManager<OsFileSystem>, dir: &Path) {
    let api = manager
        .load_global_config()
        .unwrap()
        .api_configs
        .remove("protected")
        .unwrap();
    assert!(api.fetch_auth.is_some());
    assert!(api.secrets.contains_key("operation-key"));
    for path in [
        dir.join("config.toml"),
        dir.join("specs/protected.yaml"),
        dir.join(".cache/protected.bin"),
    ] {
        let bytes = std::fs::read(path).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-token"));
    }
}

#[tokio::test]
async fn explicit_no_auth_clears_reference_only_after_success() {
    let server = Server::new(|_| ok(SPEC));
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    std::env::set_var("APERTURE_FETCH_CLEAR", "synthetic");
    let args = FetchAuthArgs {
        fetch_auth: Some(FetchMethod::Bearer),
        fetch_auth_env: Some("APERTURE_FETCH_CLEAR".into()),
        fetch_header_name: None,
    };
    register(&manager, &server, &args).await.unwrap();
    let other = Server::new(|_| ok(SPEC));
    assert!(register(&manager, &other, &FetchAuthArgs::default())
        .await
        .is_err());
    assert!(other.requests.lock().unwrap().is_empty());
    let none = FetchAuthArgs {
        fetch_auth: Some(FetchMethod::None),
        ..Default::default()
    };
    let invalid = Server::new(|_| ok("not an OpenAPI spec"));
    assert!(register(&manager, &invalid, &none).await.is_err());
    assert!(
        manager.load_global_config().unwrap().api_configs["protected"]
            .fetch_auth
            .is_some()
    );
    register(&manager, &other, &none).await.unwrap();
    assert!(
        manager.load_global_config().unwrap().api_configs["protected"]
            .fetch_auth
            .is_none()
    );
    assert!(!other
        .requests
        .lock()
        .unwrap()
        .last()
        .unwrap()
        .contains("authorization"));
    std::env::remove_var("APERTURE_FETCH_CLEAR");
}

struct FailFs {
    writes: std::sync::atomic::AtomicUsize,
    fail_at: std::sync::atomic::AtomicUsize,
    persistent: bool,
}

impl FileSystem for FailFs {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        OsFileSystem.read_to_string(path)
    }
    fn write_all(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        OsFileSystem.write_all(path, bytes)
    }
    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        OsFileSystem.create_dir_all(path)
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        OsFileSystem.remove_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> std::io::Result<()> {
        OsFileSystem.remove_dir_all(path)
    }
    fn exists(&self, path: &Path) -> bool {
        OsFileSystem.exists(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        OsFileSystem.is_dir(path)
    }
    fn is_file(&self, path: &Path) -> bool {
        OsFileSystem.is_file(path)
    }
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        OsFileSystem.canonicalize(path)
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<PathBuf>> {
        OsFileSystem.read_dir(path)
    }
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let number = self
            .writes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        let fail_at = self.fail_at.load(std::sync::atomic::Ordering::Relaxed);
        if fail_at != 0 && (number == fail_at || (self.persistent && number >= fail_at)) {
            return Err(std::io::Error::other(
                "private-storage-secret: synthetic write failure",
            ));
        }
        OsFileSystem.atomic_write(path, bytes)
    }
}

#[test]
fn handled_write_failures_restore_registration_and_operation_mappings() {
    for fail_at in 1..=4 {
        rollback_replacement(fail_at);
    }
}

fn rollback_replacement(fail_at: usize) {
    let dir = tempfile::tempdir().unwrap();
    let fs = FailFs {
        writes: 0.into(),
        persistent: false,
        fail_at: 0.into(),
    };
    let manager = ConfigManager::with_fs(fs, dir.path().into());
    let name = ApiContextName::new("protected").unwrap();
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "SYNTHETIC_REF",
        None,
        "https://example.com/spec",
    )
    .unwrap();
    manager
        .register_fetched_spec(&name, SPEC, false, Some(auth))
        .unwrap();
    manager.set_secret(&name, "operation", "OP_TOKEN").unwrap();
    let paths = [
        "specs/protected.yaml",
        ".cache/protected.bin",
        ".cache/cache_metadata.json",
        "config.toml",
    ];
    let paths = paths.map(|path| dir.path().join(path));
    let before = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    manager
        .fs
        .writes
        .store(0, std::sync::atomic::Ordering::Relaxed);
    manager
        .fs
        .fail_at
        .store(fail_at, std::sync::atomic::Ordering::Relaxed);
    let replacement = SPEC.replace("Protected", "Replacement");
    assert!(manager
        .register_fetched_spec(&name, &replacement, true, None)
        .is_err());
    let after = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    assert_eq!(before, after);
}

#[test]
fn handled_initial_registration_failures_leave_no_registered_files() {
    for fail_at in 1..=5 {
        rollback_initial(fail_at);
    }
}

fn rollback_initial(fail_at: usize) {
    let dir = tempfile::tempdir().unwrap();
    let fs = FailFs {
        writes: 0.into(),
        persistent: false,
        fail_at: fail_at.into(),
    };
    let manager = ConfigManager::with_fs(fs, dir.path().into());
    let name = ApiContextName::new("protected").unwrap();
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "SYNTHETIC_REF",
        None,
        "https://example.com",
    )
    .unwrap();
    assert!(manager
        .register_fetched_spec(&name, SPEC, false, Some(auth))
        .is_err());
    assert!(manager.list_specs().unwrap().is_empty());
    assert!(!dir.path().join("config.toml").exists());
    assert!(!dir.path().join(".cache/protected.bin").exists());
    assert!(!dir.path().join(".cache/cache_metadata.json").exists());
}

#[test]
fn removal_clears_fetch_reference_but_preserves_operation_settings() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let name = ApiContextName::new("protected").unwrap();
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "SYNTHETIC_REF",
        None,
        "https://example.com",
    )
    .unwrap();
    manager
        .register_fetched_spec(&name, SPEC, false, Some(auth))
        .unwrap();
    manager.set_secret(&name, "operation", "OP_TOKEN").unwrap();
    manager.remove_spec(&name).unwrap();
    let api = manager
        .load_global_config()
        .unwrap()
        .api_configs
        .remove("protected")
        .unwrap();
    assert!(api.fetch_auth.is_none());
    assert!(api.secrets.contains_key("operation"));
}

#[tokio::test]
async fn reflected_authenticated_diagnostics_preserve_replacement() {
    for (method, header, value, reflected) in [
        (
            FetchMethod::Basic,
            None,
            "user:synthetic-private",
            "Basic dXNlcjpzeW50aGV0aWMtcHJpdmF0ZQ==",
        ),
        (
            FetchMethod::Basic,
            None,
            "user:synthetic-private",
            "user:synthetic-private",
        ),
        (
            FetchMethod::Basic,
            None,
            "user:synthetic-private",
            "synthetic-private",
        ),
        (
            FetchMethod::Bearer,
            None,
            "synthetic-private",
            "Bearer synthetic-private",
        ),
        (
            FetchMethod::Header,
            Some("X-Key"),
            "synthetic-private",
            "synthetic-private",
        ),
    ] {
        check_reflected_diagnostic(method, header, value, reflected).await;
    }
}

async fn check_reflected_diagnostic(
    method: FetchMethod,
    header: Option<&str>,
    value: &str,
    reflected: &str,
) {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let name = ApiContextName::new("protected").unwrap();
    manager
        .register_fetched_spec(&name, SPEC, false, None)
        .unwrap();
    manager.set_secret(&name, "operation", "OP_TOKEN").unwrap();
    let paths = [
        "specs/protected.yaml",
        ".cache/protected.bin",
        ".cache/cache_metadata.json",
        "config.toml",
    ];
    let before = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
    let bodies = [
        serde_json::json!({"openapi":"3.0.3", "info":reflected, "paths":{}}).to_string(),
        format!("{SPEC}components:\n  securitySchemes:\n    '{reflected}':\n      type: apiKey\n      in: header\n      name: X-Key\n      x-aperture-secret:\n        source: file\n"),
        format!("openapi: 3.0.3\ninfo: {{title: Protected, version: '1'}}\npaths:\n  /test:\n    get:\n      operationId: test\n      parameters:\n        - $ref: '{reflected}'\n      responses: {{}}\n"),
    ];
    std::env::set_var("APERTURE_FETCH_REFLECTION", value);
    for body in bodies {
        let served_body = body.clone();
        let server = Server::new(move |_| ok(&served_body));
        let args = FetchAuthArgs {
            fetch_auth: Some(method),
            fetch_auth_env: Some("APERTURE_FETCH_REFLECTION".into()),
            fetch_header_name: header.map(str::to_owned),
        };
        let error = register(&manager, &server, &args).await.unwrap_err();
        assert!(!format!("{error:?} {error}").contains(reflected));
        let auth = args.select(&server.url, None).unwrap();
        let error = manager
            .register_fetched_spec(&name, &body, false, auth)
            .unwrap_err();
        assert!(
            !format!("{error:?} {error}").contains(reflected),
            "reflected diagnostic: {error}"
        );
        let after = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
        assert_eq!(before, after);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
    }
    std::env::remove_var("APERTURE_FETCH_REFLECTION");
}

#[tokio::test]
async fn authenticated_warning_is_safe_and_strict_rejection_preserves_files() {
    let body = "openapi: 3.0.3\ninfo: {title: Protected, version: '1'}\npaths:\n  /synthetic-warning-private:\n    post:\n      operationId: test\n      requestBody:\n        content:\n          application/xml:\n            schema: {type: object}\n      responses: {}\n";
    let server = Server::new(move |_| ok(body));
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let args = FetchAuthArgs {
        fetch_auth: Some(FetchMethod::Bearer),
        fetch_auth_env: Some("APERTURE_FETCH_WARNING".into()),
        fetch_header_name: None,
    };
    std::env::set_var("APERTURE_FETCH_WARNING", "synthetic-download-token");
    register(&manager, &server, &args).await.unwrap();
    let paths = [
        "specs/protected.yaml",
        ".cache/protected.bin",
        ".cache/cache_metadata.json",
        "config.toml",
    ];
    let before = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
    let error = register_with_strict(&manager, &server, &args, true)
        .await
        .unwrap_err();
    assert!(!format!("{error:?} {error}").contains("synthetic-warning-private"));
    let after = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
    assert_eq!(before, after);
    std::env::remove_var("APERTURE_FETCH_WARNING");
}

#[test]
fn public_and_local_parse_diagnostics_remain_detailed() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let name = ApiContextName::new("public").unwrap();
    let body = r#"{"openapi":"3.0.3","info":"public-invalid-info","paths":{}}"#;
    let remote = manager
        .register_fetched_spec(&name, body, false, None)
        .unwrap_err();
    assert!(remote.to_string().contains("public-invalid-info"));
    let path = dir.path().join("input.json");
    std::fs::write(&path, body).unwrap();
    let local = manager.add_spec(&name, &path, false, false).unwrap_err();
    assert!(local.to_string().contains("public-invalid-info"));
}

#[tokio::test]
async fn valid_reflected_credentials_are_not_persisted() {
    for reflected in [
        "user:synthetic-storage-private",
        "synthetic-storage-private",
        "Basic dXNlcjpzeW50aGV0aWMtc3RvcmFnZS1wcml2YXRl",
    ] {
        let body = SPEC.replace("Protected", reflected);
        let server = Server::new(move |_| ok(&body));
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
        let args = FetchAuthArgs {
            fetch_auth: Some(FetchMethod::Basic),
            fetch_auth_env: Some("APERTURE_FETCH_STORAGE_REFLECTION".into()),
            fetch_header_name: None,
        };
        std::env::set_var(
            "APERTURE_FETCH_STORAGE_REFLECTION",
            "user:synthetic-storage-private",
        );
        assert!(register(&manager, &server, &args).await.is_err());
        assert!(manager.list_specs().unwrap().is_empty());
        assert!(!dir.path().join("config.toml").exists());
    }
    std::env::remove_var("APERTURE_FETCH_STORAGE_REFLECTION");
}

#[tokio::test]
async fn escaped_credentials_are_rejected_before_initial_registration() {
    for method in [FetchMethod::Basic, FetchMethod::Bearer, FetchMethod::Header] {
        for yaml in [false, true] {
            let secret = "private-decoded-token";
            let escaped = escape_fixture(secret, yaml);
            let body = if yaml {
                format!(
                    "openapi: 3.0.3\ninfo: {{title: \"{escaped}\", version: '1'}}\npaths: {{}}\n"
                )
            } else {
                format!("{{\"openapi\":\"3.0.3\",\"info\":{{\"title\":\"{escaped}\",\"version\":\"1\"}},\"paths\":{{}}}}")
            };
            let server = Server::new(move |_| ok(&body));
            let dir = tempfile::tempdir().unwrap();
            let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
            std::env::set_var(
                "APERTURE_FETCH_DECODED",
                if method == FetchMethod::Basic {
                    "user:private-decoded-token"
                } else {
                    secret
                },
            );
            let args = FetchAuthArgs {
                fetch_auth: Some(method),
                fetch_auth_env: Some("APERTURE_FETCH_DECODED".into()),
                fetch_header_name: (method == FetchMethod::Header).then(|| "X-Key".into()),
            };
            assert!(register(&manager, &server, &args).await.is_err());
            assert!(!dir.path().join("specs/protected.yaml").exists());
            assert!(!dir.path().join(".cache/protected.bin").exists());
            assert!(!dir.path().join("config.toml").exists());
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }
    std::env::remove_var("APERTURE_FETCH_DECODED");
}

#[test]
fn authenticated_storage_failures_preserve_safe_categories_and_rollback_notice() {
    for persistent in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::with_fs(
            FailFs {
                writes: 0.into(),
                fail_at: 0.into(),
                persistent,
            },
            dir.path().into(),
        );
        let name = ApiContextName::new("protected").unwrap();
        let auth = FetchAuth::new(
            FetchMethod::Bearer,
            "STORAGE_SYNTH",
            None,
            "https://example.com",
        )
        .unwrap();
        manager
            .register_fetched_spec(&name, SPEC, false, Some(auth.clone()))
            .unwrap();
        let paths = [
            "specs/protected.yaml",
            ".cache/protected.bin",
            ".cache/cache_metadata.json",
            "config.toml",
        ];
        let before = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
        manager
            .fs
            .writes
            .store(0, std::sync::atomic::Ordering::Relaxed);
        manager
            .fs
            .fail_at
            .store(2, std::sync::atomic::Ordering::Relaxed);
        let error = manager
            .register_fetched_spec(
                &name,
                &SPEC.replace("Protected", "Replacement"),
                false,
                Some(auth),
            )
            .unwrap_err();
        let json = serde_json::to_value(error.to_json()).unwrap();
        let diagnostic = format!("{error} {error:?} {json}");
        assert!(!diagnostic.contains("synthetic write failure"));
        assert!(!diagnostic.contains("private-storage-secret"));
        assert!(diagnostic.contains("Runtime"));
        if persistent {
            assert!(matches!(error, Error::RegistrationRollbackFailed));
            assert!(diagnostic.contains("previous state may be incomplete"));
            assert_eq!(json["details"]["rollback_failed"], true);
            assert_ne!(before[0], std::fs::read(dir.path().join(paths[0])).unwrap());
        } else {
            let after = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
            assert_eq!(before, after);
        }
    }
}

#[tokio::test]
async fn hermetic_fixture_still_rejects_untrusted_certificates_and_wrong_names() {
    let server = Server::new(|_| ok(SPEC));
    let untrusted = reqwest::Client::builder()
        .no_proxy()
        .tls_certs_only([])
        .resolve("localhost", "127.0.0.1:0".parse().unwrap());
    assert!(fetch_spec_with_builder(
        &server.url,
        std::time::Duration::from_secs(3),
        None,
        untrusted
    )
    .await
    .is_err());
    let wrong_name = server.url.replace("localhost", "wrong-name.invalid");
    assert!(fetch_spec_with_builder(
        &wrong_name,
        std::time::Duration::from_secs(3),
        None,
        Server::builder().resolve("wrong-name.invalid", "127.0.0.1:0".parse().unwrap())
    )
    .await
    .is_err());
    assert!(server.requests.lock().unwrap().is_empty());
}

fn escaped_boundary_documents() -> Vec<String> {
    let mut documents = Vec::new();
    for version in ["3.0.3", "3.1.0"] {
        let fields = [
            r#""info":{"title":"SECRET","version":"1"},"paths":{}"#,
            r#""info":{"title":"Safe","version":"1"},"paths":{"/SECRET":{"get":{"description":"safe","responses":{}}}}"#,
            r#""info":{"title":"Safe","version":"1"},"paths":{"/safe":{"get":{"description":"SECRET","responses":{}}}}"#,
            r#""info":{"title":"Safe","version":"1"},"paths":{},"x-SECRET":{"safe":"safe"}"#,
            r#""info":{"title":"Safe","version":"1"},"paths":{},"x-data":{"value":"SECRET"}"#,
            r#""info":{"title":"Safe","version":"1"},"paths":{},"components":{"schemas":{"Example":{"type":"string","example":"SECRET"}}}"#,
        ];
        let json_escape = escape_fixture("private-boundary-token", false);
        for fields in fields {
            documents.push(
                format!("{{\"openapi\":\"{version}\",{fields}}}").replace("SECRET", &json_escape),
            );
        }
        let yaml_escape = escape_fixture("private-boundary-token", true);
        documents.push(format!("{{openapi: {version}, info: {{title: Safe, version: '1'}}, paths: {{}}, x-extra: {{\"{yaml_escape}\": safe}}}}"));
        documents.push(format!("openapi: {version}\ninfo: {{title: Safe, version: '1'}}\npaths: {{}}\nx-extra: [\"{yaml_escape}\"]\n"));
    }
    documents
}

#[tokio::test]
async fn decoded_keys_values_and_parser_modes_preserve_replacements() {
    std::env::set_var("APERTURE_FETCH_BOUNDARY", "private-boundary-token");
    for body in escaped_boundary_documents() {
        let server = Server::new(move |_| ok(&body));
        let dir = tempfile::tempdir().unwrap();
        let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
        let name = ApiContextName::new("protected").unwrap();
        manager
            .register_fetched_spec(&name, SPEC, false, None)
            .unwrap();
        manager.set_secret(&name, "operation", "OP_TOKEN").unwrap();
        let paths = [
            "specs/protected.yaml",
            ".cache/protected.bin",
            ".cache/cache_metadata.json",
            "config.toml",
        ];
        let before = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
        let args = FetchAuthArgs {
            fetch_auth: Some(FetchMethod::Bearer),
            fetch_auth_env: Some("APERTURE_FETCH_BOUNDARY".into()),
            fetch_header_name: None,
        };
        let error = register(&manager, &server, &args).await.unwrap_err();
        assert!(!format!("{error} {error:?}").contains("private-boundary-token"));
        let after = paths.map(|path| std::fs::read(dir.path().join(path)).unwrap());
        assert_eq!(before, after);
    }
    std::env::remove_var("APERTURE_FETCH_BOUNDARY");
}

#[test]
fn final_cache_guard_rejects_post_transformation_reflections_before_writes() {
    std::env::set_var("APERTURE_FETCH_CACHE_BOUNDARY", "private-cache-token");
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "APERTURE_FETCH_CACHE_BOUNDARY",
        None,
        "https://example.com",
    )
    .unwrap();
    let credentials = auth.resolve_download().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let spec = crate::spec::parse_openapi(SPEC).unwrap();
    let validation = SpecValidator::new().validate_with_mode(&spec, false);
    // The cache name is not a response field: this specifically exercises the final
    // persistence boundary rather than the earlier document reflection check.
    let error = manager
        .add_spec_from_validated_openapi(
            "private-cache-token",
            &spec,
            SPEC,
            &validation,
            false,
            RegistrationAuth {
                reference: Some(auth),
                credentials: Some(&credentials),
            },
        )
        .unwrap_err();
    assert!(!format!("{error} {error:?}").contains("private-cache-token"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    std::env::remove_var("APERTURE_FETCH_CACHE_BOUNDARY");
}

fn escape_fixture(value: &str, yaml: bool) -> String {
    use std::fmt::Write as _;
    let mut escaped = String::new();
    for character in value.chars() {
        if yaml {
            write!(escaped, "\\x{:02x}", u32::from(character)).unwrap();
        } else {
            write!(escaped, "\\u{:04x}", u32::from(character)).unwrap();
        }
    }
    escaped
}

#[tokio::test]
async fn authenticated_oauth_download_preserves_anonymous_overrides_offline() {
    let operations = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
        .mount(&operations)
        .await;
    for yaml in [false, true] {
        combined_auth_registration(&operations.uri(), yaml).await;
    }
    let requests = operations.received_requests().await.unwrap();
    assert_eq!(requests.len(), 8);
    for request in requests {
        match request.url.path() {
            "/anonymous" => assert!(!request.headers.contains_key("authorization")),
            "/protected" => assert_eq!(
                request.headers["authorization"],
                "Bearer synthetic-independent-operation-token"
            ),
            other => panic!("unexpected operation/flow URL: {other}"),
        }
    }
}

async fn combined_auth_registration(operation_url: &str, yaml: bool) {
    let version = if cfg!(feature = "openapi31") {
        "3.1.0"
    } else {
        "3.0.3"
    };
    let document = serde_json::json!({
        "openapi": version, "info": {"title": "Combined auth", "version": "1"},
        "servers": [{"url": operation_url}],
        "components": {"securitySchemes": {"oauth": {
            "type": "oauth2", "flows": {"clientCredentials": {
                "tokenUrl": format!("{operation_url}/token"), "scopes": {"read": "Read"}
            }}, "x-aperture-secret": {"source": "env", "name": "APERTURE_COMBINED_UNUSED"}
        }}},
        "security": [{"oauth": ["read"]}],
        "paths": {
            "/anonymous": {"get": {"operationId": "getAnonymous", "tags": ["records"],
                "security": [], "responses": {"200": {"description": "OK"}}}},
            "/protected": {"get": {"operationId": "getProtected", "tags": ["records"],
                "responses": {"200": {"description": "OK"}}}}
        }
    });
    let content = if yaml {
        serde_yaml::to_string(&document).unwrap()
    } else {
        document.to_string()
    };
    let server = Server::new(move |_| ok(&content));
    let dir = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, dir.path().into());
    let name = ApiContextName::new("protected").unwrap();
    std::env::set_var(
        "APERTURE_COMBINED_FETCH",
        "synthetic-independent-fetch-token",
    );
    let args = FetchAuthArgs {
        fetch_auth: Some(FetchMethod::Bearer),
        fetch_auth_env: Some("APERTURE_COMBINED_FETCH".into()),
        fetch_header_name: None,
    };
    register(&manager, &server, &args).await.unwrap();
    manager
        .set_secret(&name, "oauth", "APERTURE_COMBINED_OPERATION")
        .unwrap();
    std::env::remove_var("APERTURE_COMBINED_FETCH");
    for rebuilt in [false, true] {
        if rebuilt {
            manager
                .reinit_local_spec(&name, &dir.path().join("specs/protected.yaml"), false)
                .unwrap();
        }
        assert_combined_discovery_and_execution(&manager, dir.path()).await;
    }
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("authorization: Bearer synthetic-independent-fetch-token"));
    assert!(!requests[0].contains("synthetic-independent-operation-token"));
    drop(requests);
    assert!(
        manager.load_global_config().unwrap().api_configs["protected"]
            .fetch_auth
            .is_some()
    );
}

async fn assert_combined_discovery_and_execution(
    manager: &ConfigManager<OsFileSystem>,
    dir: &Path,
) {
    let cache = loader::load_cached_spec(dir.join(".cache"), "protected").unwrap();
    let spec = crate::spec::parser::parse_openapi(
        &std::fs::read_to_string(dir.join("specs/protected.yaml")).unwrap(),
    )
    .unwrap();
    for output in [
        crate::agent::generate_capability_manifest(&cache, None).unwrap(),
        crate::agent::generate_capability_manifest_from_openapi("protected", &spec, &cache, None)
            .unwrap(),
    ] {
        let manifest: serde_json::Value = serde_json::from_str(&output).unwrap();
        let commands = manifest["commands"]["records"].as_array().unwrap();
        let anonymous = commands
            .iter()
            .find(|command| command["name"] == "get-anonymous")
            .unwrap();
        assert!(anonymous.get("security_requirements").is_none());
        assert!(anonymous.get("security_scopes").is_none());
        assert_eq!(manifest["security_schemes"]["oauth"]["type"], "oauth2");
        let protected = commands
            .iter()
            .find(|command| command["name"] == "get-protected")
            .unwrap();
        assert_eq!(
            protected["security_scopes"][0]["oauth"],
            serde_json::json!(["read"])
        );
    }
    for path in [
        "config.toml",
        "specs/protected.yaml",
        ".cache/protected.bin",
    ] {
        let bytes = std::fs::read(dir.join(path)).unwrap();
        for secret in [
            "synthetic-independent-fetch-token",
            "synthetic-independent-operation-token",
        ] {
            assert!(!bytes
                .windows(secret.len())
                .any(|part| part == secret.as_bytes()));
        }
    }
    let config = manager.load_global_config().unwrap();
    std::env::remove_var("APERTURE_COMBINED_OPERATION");
    invoke_combined_command(&cache, &config, "get-anonymous").await;
    std::env::set_var(
        "APERTURE_COMBINED_OPERATION",
        "synthetic-independent-operation-token",
    );
    invoke_combined_command(&cache, &config, "get-protected").await;
    std::env::remove_var("APERTURE_COMBINED_OPERATION");
}

async fn invoke_combined_command(
    cache: &crate::cache::models::CachedSpec,
    config: &GlobalConfig,
    command: &'static str,
) {
    let matches = clap::Command::new("api")
        .subcommand(clap::Command::new("records").subcommand(clap::Command::new(command)))
        .get_matches_from(["api", "records", command]);
    crate::engine::executor::execute_request(
        cache,
        &matches,
        None,
        false,
        None,
        Some(config),
        &crate::cli::OutputFormat::Json,
        None,
        None,
        false,
        None,
    )
    .await
    .unwrap();
}
