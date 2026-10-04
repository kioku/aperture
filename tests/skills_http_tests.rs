use assert_cmd::Command;
use tempfile::TempDir;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn cli(root: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("aperture"));
    cmd.env("APERTURE_CONFIG_DIR", root.path());
    cmd
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn url_import_provenance_redirects_and_safe_errors() {
    let server = MockServer::start().await;
    let root = TempDir::new().unwrap();
    Mock::given(path("/skill"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("---\nname: remote\n---\n# Remote\n"),
        )
        .mount(&server)
        .await;
    let url = format!("{}/skill?token=private#fragment", server.uri());
    cli(&root)
        .args(["skills", "install", &url])
        .assert()
        .success();
    let output = cli(&root)
        .args(["skills", "get", "remote", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(&format!("{}/skill", server.uri())));
    assert!(!output.contains("private"));
    assert!(!output.contains("fragment"));
    let requests = server.received_requests().await.unwrap();
    assert!(!requests[0].headers.contains_key("authorization"));
    assert!(!requests[0].headers.contains_key("cookie"));
    Mock::given(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/skill"))
        .mount(&server)
        .await;
    cli(&root)
        .args([
            "skills",
            "install",
            &format!("{}/redirect", server.uri()),
            "--name",
            "redirected",
        ])
        .assert()
        .success();
    Mock::given(path("/secret-redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "Location",
            "http://user:supersecret@127.0.0.1/target?token=private",
        ))
        .mount(&server)
        .await;
    let assertion = cli(&root)
        .args([
            "--json-errors",
            "skills",
            "install",
            &format!("{}/secret-redirect?token=private", server.uri()),
            "--name",
            "unsafe",
        ])
        .assert()
        .failure();
    let errors = String::from_utf8_lossy(&assertion.get_output().stderr);
    assert!(!errors.contains("supersecret"));
    assert!(!errors.contains("private"));
    Mock::given(path("/loop"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/loop"))
        .mount(&server)
        .await;
    cli(&root)
        .args([
            "skills",
            "install",
            &format!("{}/loop", server.uri()),
            "--name",
            "loop",
        ])
        .assert()
        .failure();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_failures_are_bounded_and_do_not_install() {
    let server = MockServer::start().await;
    let root = TempDir::new().unwrap();
    for (route, response) in [
        (
            "/status",
            ResponseTemplate::new(403).set_body_string("private body"),
        ),
        (
            "/utf8",
            ResponseTemplate::new(200).set_body_bytes(vec![0xff]),
        ),
        (
            "/large",
            ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 1024 * 1024 + 1]),
        ),
        ("/empty", ResponseTemplate::new(200).set_body_string("")),
    ] {
        Mock::given(path(route))
            .respond_with(response)
            .mount(&server)
            .await;
        let assertion = cli(&root)
            .args([
                "--json-errors",
                "skills",
                "install",
                &format!("{}{route}?token=private", server.uri()),
                "--name",
                "failed",
            ])
            .assert()
            .failure();
        let errors = String::from_utf8_lossy(&assertion.get_output().stderr);
        assert!(!errors.contains("private"));
        assert!(!root.path().join("skills/failed").exists());
    }
    // Bind then release a local port: deterministic connection refusal without a live service.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    cli(&root)
        .args([
            "skills",
            "install",
            &format!("http://{address}/skill?token=private"),
            "--name",
            "failed",
        ])
        .assert()
        .failure()
        .stderr(predicates::str::contains("Skill download failed"));
}
