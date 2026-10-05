//! Local synthetic HTTPS transport tests; the fixture root is trusted only by these clients.
use super::{ensure_tls_provider, operation_redirect_policy};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

const CERT: &[u8] = include_bytes!("../../tests/fixtures/fetch-auth/cert.pem");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/fetch-auth/key.pem");

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn new(response: impl Fn(&str) -> String + Send + 'static) -> Self {
        ensure_tls_provider();
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

fn read_request(stream: &mut impl Read) -> Option<String> {
    let mut request = Vec::new();
    let mut byte = [0];
    while !request.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).ok()?;
        request.push(byte[0]);
    }
    String::from_utf8(request).ok()
}

fn redirect(target: &str) -> String {
    format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

#[tokio::test]
async fn verified_https_rejects_unsafe_targets_before_delivery() {
    let target = Server::new(|_| panic!("unsafe destination received request"));
    let http_target = wiremock::MockServer::start().await;
    for destination in [
        format!("{}/target", target.url),
        target.url.replace("localhost", "127.0.0.1"),
        http_target.uri(),
    ] {
        let source = Server::new(move |_| redirect(&destination));
        let client = Server::builder()
            .redirect(operation_redirect_policy())
            .build()
            .unwrap();
        let error = client
            .post(&source.url)
            .header("X-Service-Key", "synthetic-key")
            .send()
            .await
            .unwrap_err()
            .without_url();
        assert!(error.is_redirect());
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        assert!(target.requests.lock().unwrap().is_empty());
        assert!(http_target.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn verified_https_same_origin_retains_headers_and_bounds_loops() {
    let server = Server::new(|request| {
        if request.starts_with("GET /start ") {
            redirect("/finish")
        } else {
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()
        }
    });
    let client = Server::builder()
        .redirect(operation_redirect_policy())
        .build()
        .unwrap();
    assert!(client
        .get(format!("{}/start", server.url))
        .header("X-Service-Key", "synthetic-key")
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|r| r.to_lowercase().contains("x-service-key: synthetic-key")));
    let looping = Server::new(|_| redirect("/loop"));
    assert!(client
        .get(&looping.url)
        .send()
        .await
        .unwrap_err()
        .is_redirect());
    assert_eq!(looping.requests.lock().unwrap().len(), 10);
}

#[tokio::test]
async fn userinfo_is_rejected_even_on_same_origin() {
    let server = Server::new(|request| {
        let host = request
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .unwrap();
        redirect(&format!(
            "https://synthetic-user:synthetic-password@{host}/secret"
        ))
    });
    let client = Server::builder()
        .redirect(operation_redirect_policy())
        .build()
        .unwrap();
    let error = client
        .get(&server.url)
        .send()
        .await
        .unwrap_err()
        .without_url();
    assert!(error.is_redirect());
    assert_eq!(server.requests.lock().unwrap().len(), 1);
    assert!(!format!("{error:?}").contains("synthetic-password"));
}
