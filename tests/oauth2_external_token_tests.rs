mod test_helpers;

use aperture_cli::agent::{
    generate_capability_manifest, generate_capability_manifest_from_openapi,
};
use aperture_cli::cache::models::CachedSpec;
use aperture_cli::cli::OutputFormat;
use aperture_cli::config::models::{ApertureSecret, ApiConfig, GlobalConfig, SecretSource};
use aperture_cli::engine::executor::execute_request;
use aperture_cli::spec::{
    parser::parse_openapi, transformer::SpecTransformer, validator::SpecValidator,
};
use clap::Command;
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn document(url: &str, token_env: &str) -> Value {
    json!({
        "openapi": "3.0.3", "info": {"title": "External OAuth", "version": "1"},
        "servers": [{"url": url}],
        "components": {"securitySchemes": {
            "oauth": {"type": "oauth2", "flows": {
                "authorizationCode": {"authorizationUrl": format!("{url}/authorize"),
                    "tokenUrl": format!("{url}/token"), "refreshUrl": format!("{url}/refresh"),
                    "scopes": {"read": "Read records", "write": "Write records"}},
                "clientCredentials": {"tokenUrl": format!("{url}/token"), "scopes": {"read": "Read records"}},
                "implicit": {"authorizationUrl": format!("{url}/authorize"), "scopes": {"read": "Read records"}},
                "password": {"tokenUrl": format!("{url}/token"), "scopes": {"read": "Read records"}}
            }, "x-aperture-secret": {"source": "env", "name": token_env}},
            "key": {"type": "apiKey", "in": "header", "name": "X-Key",
                "x-aperture-secret": {"source": "env", "name": "OAUTH259_KEY"}},
            "missing": {"type": "http", "scheme": "bearer",
                "x-aperture-secret": {"source": "env", "name": "OAUTH259_ABSENT"}}
        }},
        "security": [{"oauth": ["read"]}],
        "paths": {"/records": {"get": {"operationId": "getRecords", "tags": ["records"],
            "responses": {"200": {"description": "OK"}}}}}
    })
}

fn cached(doc: &Value) -> (openapiv3::OpenAPI, CachedSpec) {
    let spec = parse_openapi(&doc.to_string()).unwrap();
    for strict in [false, true] {
        assert!(SpecValidator::new()
            .validate_with_mode(&spec, strict)
            .into_result()
            .is_ok());
    }
    let cache = SpecTransformer::new()
        .transform("oauth-test", &spec)
        .unwrap();
    (spec, cache)
}

#[test]
fn oauth_metadata_survives_postcard_and_both_discovery_paths() {
    let doc = document("http://127.0.0.1:1", "OAUTH259_METADATA");
    std::env::set_var("OAUTH259_METADATA", "synthetic-metadata-secret");
    let (spec, cache) = cached(&doc);
    let bytes = postcard::to_allocvec(&cache).unwrap();
    assert!(!bytes
        .windows(b"synthetic-metadata-secret".len())
        .any(|part| part == b"synthetic-metadata-secret"));
    let restored: CachedSpec = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(cache, restored);
    assert_eq!(restored.security_schemes["oauth"].scheme_type, "oauth2");
    assert_eq!(
        restored.commands[0].security_scopes[0]["oauth"],
        vec!["read"]
    );
    for output in [
        generate_capability_manifest(&restored, None).unwrap(),
        generate_capability_manifest_from_openapi("oauth-test", &spec, &restored, None).unwrap(),
    ] {
        assert!(!output.contains("synthetic-metadata-secret"));
        let manifest: Value = serde_json::from_str(&output).unwrap();
        let details = &manifest["security_schemes"]["oauth"];
        assert_eq!(
            details["flows"],
            doc["components"]["securitySchemes"]["oauth"]["flows"]
        );
        assert_eq!(details["execution_mode"], "externalBearerToken");
        assert_eq!(details["token_grants_verified"], false);
        let command = &manifest["commands"]["records"][0];
        assert_eq!(command["security_scopes"][0]["oauth"], json!(["read"]));
    }
    std::env::remove_var("OAUTH259_METADATA");
}

async fn invoke(
    cache: &CachedSpec,
    config: Option<&GlobalConfig>,
    dry_run: bool,
) -> Result<Option<String>, aperture_cli::error::Error> {
    let matches = Command::new("api")
        .subcommand(Command::new("records").subcommand(Command::new("get-records")))
        .get_matches_from(["api", "records", "get-records"]);
    execute_request(
        cache,
        &matches,
        None,
        dry_run,
        None,
        config,
        &OutputFormat::Json,
        None,
        None,
        false,
        None,
    )
    .await
}

#[tokio::test]
async fn oauth_executes_alternatives_combined_overrides_and_anonymous_without_token_requests() {
    let server = MockServer::start().await;
    let env = "OAUTH259_EXEC_TOKEN";
    std::env::set_var(env, "synthetic-external-token");
    std::env::set_var("OAUTH259_KEY", "synthetic-key");
    std::env::set_var("OAUTH259_OVERRIDE", "synthetic-override");
    std::env::remove_var("OAUTH259_ABSENT");
    Mock::given(method("GET"))
        .and(path("/records"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok": true})))
        .mount(&server)
        .await;
    let mut doc = document(&server.uri(), env);
    for security in [
        json!([{"oauth": ["read"]}]),
        json!([{"missing": []}, {"oauth": ["write"]}]),
        json!([{"oauth": ["read"], "key": []}]),
    ] {
        doc["paths"]["/records"]["get"]["security"] = security;
        let (_, cache) = cached(&doc);
        invoke(&cache, None, false).await.unwrap();
    }
    let (_, cache) = cached(&doc);
    let mut config = GlobalConfig::default();
    let mut api = ApiConfig {
        base_url_override: None,
        environment_urls: std::collections::HashMap::new(),
        secrets: std::collections::HashMap::new(),
        strict_mode: false,
        command_mapping: None,
    };
    api.secrets.insert(
        "oauth".into(),
        ApertureSecret {
            source: SecretSource::Env,
            name: "OAUTH259_OVERRIDE".into(),
        },
    );
    config.api_configs.insert("oauth-test".into(), api);
    invoke(&cache, Some(&config), false).await.unwrap();
    invoke(&cache, Some(&config), true).await.unwrap();
    std::env::remove_var(env);
    assert!(invoke(&cache, None, false).await.is_err());
    doc["paths"]["/records"]["get"]["security"] = json!([{"oauth": ["read"]}, {}]);
    let (_, anonymous) = cached(&doc);
    invoke(&anonymous, None, false).await.unwrap();
    doc["paths"]["/records"]["get"]["security"] = json!([{"oauth": ["read"]}, {"key": []}]);
    let (_, fallback) = cached(&doc);
    invoke(&fallback, None, false).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 6);
    for request in &requests {
        assert_eq!(request.url.path(), "/records");
    }
    for request in &requests[..3] {
        assert_eq!(
            request.headers["authorization"],
            "Bearer synthetic-external-token"
        );
    }
    assert_eq!(requests[2].headers["x-key"], "synthetic-key");
    assert_eq!(
        requests[3].headers["authorization"],
        "Bearer synthetic-override"
    );
    assert!(!requests[4].headers.contains_key("authorization"));
    assert!(!requests[5].headers.contains_key("authorization"));
    assert_eq!(requests[5].headers["x-key"], "synthetic-key");
    std::env::remove_var("OAUTH259_KEY");
    std::env::remove_var("OAUTH259_OVERRIDE");
}

#[tokio::test]
async fn oauth_invalid_tokens_and_authentication_failure_do_not_refresh_or_leak() {
    let server = MockServer::start().await;
    let env = "OAUTH259_INVALID_TOKEN";
    let (_, cache) = cached(&document(&server.uri(), env));
    std::env::set_var(env, "synthetic-secret\ninvalid");
    let error = invoke(&cache, None, false).await.unwrap_err().to_string();
    assert!(!error.contains("synthetic-secret"));
    assert!(server.received_requests().await.unwrap().is_empty());
    std::env::set_var(env, "synthetic-rejected-token");
    Mock::given(method("GET"))
        .and(path("/records"))
        .and(header("Authorization", "Bearer synthetic-rejected-token"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let error = invoke(&cache, None, false).await.unwrap_err().to_string();
    assert!(!error.contains("synthetic-rejected-token"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    std::env::remove_var(env);
}

#[test]
fn oauth_operation_overrides_and_malformed_documents_fail_safely() {
    let mut doc = document("http://127.0.0.1:1", "OAUTH259_UNUSED");
    doc["paths"]["/records"]["get"]["security"] = json!([]);
    let (_, cache) = cached(&doc);
    assert!(cache.commands[0].security_requirements.is_empty());
    assert!(cache.commands[0].security_scopes.is_empty());
    doc["paths"]["/records"]["get"]["security"] = json!([{"unknown": ["read"]}]);
    let spec = parse_openapi(&doc.to_string()).unwrap();
    assert!(SpecValidator::new()
        .validate_with_mode(&spec, true)
        .into_result()
        .is_err());
    doc["components"]["securitySchemes"]["oauth"]["flows"]["authorizationCode"]
        .as_object_mut()
        .unwrap()
        .remove("tokenUrl");
    assert!(parse_openapi(&doc.to_string()).is_err());
}

#[test]
fn oauth_yaml_parser_preserves_flow_metadata() {
    let doc = document("http://127.0.0.1:1", "OAUTH259_YAML");
    let yaml = serde_yaml::to_string(&doc).unwrap();
    let spec = parse_openapi(&yaml).unwrap();
    let cache = SpecTransformer::new()
        .transform("oauth-test", &spec)
        .unwrap();
    let flows: Value = serde_json::from_str(
        cache.security_schemes["oauth"]
            .oauth2_flows
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        flows,
        doc["components"]["securitySchemes"]["oauth"]["flows"]
    );
}

#[cfg(feature = "openapi31")]
#[test]
fn oauth_openapi31_parser_preserves_flows_and_required_scopes() {
    let mut doc = document("http://127.0.0.1:1", "OAUTH259_OAS31");
    doc["openapi"] = json!("3.1.0");
    let (_, cache) = cached(&doc);
    let flows: Value = serde_json::from_str(
        cache.security_schemes["oauth"]
            .oauth2_flows
            .as_deref()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        flows,
        doc["components"]["securitySchemes"]["oauth"]["flows"]
    );
    assert_eq!(cache.commands[0].security_scopes[0]["oauth"], vec!["read"]);
}

#[test]
fn oauth_extension_validation_and_openid_connect_remain_safe() {
    let mut doc = document("http://127.0.0.1:1", "OAUTH259_SAFE");
    for extension in [
        json!({"source": "env", "name": "bad-name"}),
        json!({"source": "file", "name": "OAUTH259_SAFE"}),
        json!({"source": "env"}),
    ] {
        doc["components"]["securitySchemes"]["oauth"]["x-aperture-secret"] = extension;
        let spec = parse_openapi(&doc.to_string()).unwrap();
        for strict in [false, true] {
            assert!(SpecValidator::new()
                .validate_with_mode(&spec, strict)
                .into_result()
                .is_err());
        }
    }
    doc["components"]["securitySchemes"]["oauth"] =
        json!({"type": "openIdConnect", "openIdConnectUrl": "http://127.0.0.1:1/discovery"});
    let spec = parse_openapi(&doc.to_string()).unwrap();
    assert!(SpecValidator::new()
        .validate_with_mode(&spec, true)
        .into_result()
        .is_err());
    let validation = SpecValidator::new().validate_with_mode(&spec, false);
    assert_eq!(validation.warnings.len(), 1);
    doc["paths"]["/records"]["get"]["security"] = json!([{}]);
    let spec = parse_openapi(&doc.to_string()).unwrap();
    assert!(SpecValidator::new()
        .validate_with_mode(&spec, false)
        .warnings
        .is_empty());
}

#[cfg(feature = "integration")]
#[test]
fn oauth_cli_registration_override_redaction_and_v9_cache_rebuild() {
    let temp = tempfile::TempDir::new().unwrap();
    let config_dir = temp.path().join("config");
    let spec_path = temp.path().join("oauth.json");
    std::fs::write(
        &spec_path,
        document("http://127.0.0.1:1", "OAUTH259_CLI_EXTENSION").to_string(),
    )
    .unwrap();
    let cli = |args: &[&str]| {
        let output = std::process::Command::new(assert_cmd::cargo::cargo_bin!("aperture"))
            .env("APERTURE_CONFIG_DIR", &config_dir)
            .env("OAUTH259_CLI_EXTENSION", "synthetic-cli-extension")
            .env("OAUTH259_CLI_OVERRIDE", "synthetic-cli-override")
            .args(args)
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-cli-"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("synthetic-cli-"));
        output
    };
    assert!(cli(&[
        "config",
        "add",
        "oauth-cli",
        spec_path.to_str().unwrap(),
        "--strict"
    ])
    .status
    .success());
    assert!(cli(&[
        "config",
        "secret",
        "set",
        "oauth-cli",
        "oauth",
        "--env",
        "OAUTH259_CLI_OVERRIDE"
    ])
    .status
    .success());
    let discovery = cli(&["api", "oauth-cli", "--describe-json"]);
    assert!(
        discovery.status.success(),
        "{}",
        String::from_utf8_lossy(&discovery.stderr)
    );
    let manifest: Value = serde_json::from_slice(&discovery.stdout).unwrap();
    assert_eq!(manifest["security_schemes"]["oauth"]["type"], "oauth2");
    let dry_run = cli(&["api", "oauth-cli", "--dry-run", "records", "get-records"]);
    assert!(
        dry_run.status.success(),
        "{}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    for path in [
        config_dir.join("config.toml"),
        config_dir.join(".cache/oauth-cli.bin"),
    ] {
        if path.exists() {
            let bytes = std::fs::read(path).unwrap();
            assert!(!bytes
                .windows(b"synthetic-cli-".len())
                .any(|part| part == b"synthetic-cli-"));
        }
    }
    let cache_path = config_dir.join(".cache/oauth-cli.bin");
    std::fs::write(&cache_path, postcard::to_allocvec(&9_u32).unwrap()).unwrap();
    assert!(!cli(&["api", "oauth-cli", "--describe-json"])
        .status
        .success());
    assert!(cli(&["config", "reinit", "oauth-cli"]).status.success());
    let cache: CachedSpec = postcard::from_bytes(&std::fs::read(cache_path).unwrap()).unwrap();
    assert_eq!(
        cache.cache_format_version,
        aperture_cli::cache::models::CACHE_FORMAT_VERSION
    );
    assert_eq!(cache.commands[0].security_scopes[0]["oauth"], vec!["read"]);
    assert!(cache.security_schemes["oauth"].oauth2_flows.is_some());
}
