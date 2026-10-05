//! Local synthetic HTTPS transport tests; the fixture root is trusted only by these clients.
use super::*;
use crate::config::fetch_auth::FetchMethod;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::TcpListener;
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
            .add_root_certificate(reqwest::Certificate::from_pem(CERT).unwrap())
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
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
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
    assert!(server.requests.lock().unwrap().len() == 1);
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
    let name = ApiContextName::new("protected").unwrap();
    let config = manager.load_global_config()?;
    let saved = config
        .api_configs
        .get("protected")
        .and_then(|api| api.fetch_auth.as_ref());
    let auth = args.select(&server.url, saved)?;
    let content = fetch_spec_with_builder(
        &server.url,
        std::time::Duration::from_secs(3),
        auth.as_ref(),
        Server::builder(),
    )
    .await?;
    manager.register_fetched_spec(&name, &content, false, auth)
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
        if number == self.fail_at.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(std::io::Error::other("synthetic write failure"));
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
