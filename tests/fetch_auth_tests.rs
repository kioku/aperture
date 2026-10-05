use aperture_cli::config::fetch_auth::{FetchAuth, FetchMethod};

#[test]
fn credentials_are_environment_references_only() {
    let auth = FetchAuth::new(
        FetchMethod::Basic,
        "SPEC_CREDS",
        None,
        "https://example.com/spec",
    )
    .unwrap();
    assert!(auth.header_for_value("user:password:with:colons").is_ok());
    assert!(auth.header_for_value("missing-colon").is_err());
    assert!(auth.header_for_value("").is_err());
    assert!(!serde_json::to_string(&auth).unwrap().contains("password"));
}

#[test]
fn authenticated_targets_are_bound_to_https_origin() {
    let auth = FetchAuth::new(
        FetchMethod::Bearer,
        "SPEC_TOKEN",
        None,
        "https://example.com/spec",
    )
    .unwrap();
    for target in [
        "http://example.com/spec",
        "https://elsewhere.com/spec",
        "https://example.com:444/spec",
        "https://user:pass@example.com/spec",
    ] {
        assert!(auth.validate_target(target).is_err());
    }
    assert!(auth
        .validate_target("https://example.com:443/other")
        .is_ok());
}

#[test]
fn rejects_invalid_references_and_headers() {
    for name in ["", "1TOKEN", "TOKEN-NAME", "TOKEN\n"] {
        assert!(FetchAuth::new(FetchMethod::Bearer, name, None, "https://example.com").is_err());
    }
    for header in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Connection",
        "Authorization",
        "bad header",
    ] {
        assert!(FetchAuth::new(
            FetchMethod::Header,
            "TOKEN",
            Some(header),
            "https://example.com"
        )
        .is_err());
    }
    let auth = FetchAuth::new(
        FetchMethod::Header,
        "TOKEN",
        Some("X-API-Key"),
        "https://example.com",
    )
    .unwrap();
    assert!(auth.header_for_value("secret\r\nInjected: yes").is_err());
}

#[test]
fn basic_control_bytes_are_rejected_before_encoding() {
    let auth = FetchAuth::new(
        FetchMethod::Basic,
        "SPEC_CREDS",
        None,
        "https://example.com",
    )
    .unwrap();
    assert!(auth.header_for_value("user:password\tcontrol").is_err());
}

#[test]
fn canonical_and_alias_parse_identical_fetch_flags() {
    use aperture_cli::cli::Cli;
    use clap::Parser;
    for prefix in [
        vec!["aperture", "config", "api", "add"],
        vec!["aperture", "config", "add"],
    ] {
        let mut command = prefix.clone();
        command.extend([
            "protected",
            "https://example.com/spec",
            "--fetch-auth",
            "header",
            "--fetch-auth-env",
            "SPEC_TOKEN",
            "--fetch-header-name",
            "X-Key",
        ]);
        assert!(Cli::try_parse_from(command).is_ok());
        let mut invalid = prefix;
        invalid.extend([
            "protected",
            "https://example.com/spec",
            "--fetch-auth-env",
            "SPEC_TOKEN",
        ]);
        assert!(Cli::try_parse_from(invalid).is_err());
    }
}

#[test]
fn selection_validates_combinations_and_never_migrates_saved_origin() {
    use aperture_cli::config::fetch_auth::FetchAuthArgs;
    let saved = FetchAuth::new(FetchMethod::Bearer, "TOKEN", None, "https://example.com").unwrap();
    assert!(FetchAuthArgs::default()
        .select("https://other.example.com", Some(&saved))
        .is_err());
    for method in [FetchMethod::None, FetchMethod::Basic, FetchMethod::Bearer] {
        let flags = FetchAuthArgs {
            fetch_auth: Some(method),
            fetch_auth_env: Some("TOKEN".into()),
            fetch_header_name: Some("X-Key".into()),
        };
        assert!(flags.select("https://example.com", None).is_err());
    }
    let none = FetchAuthArgs {
        fetch_auth: Some(FetchMethod::None),
        ..Default::default()
    };
    assert!(none
        .select("http://other.example.com", Some(&saved))
        .unwrap()
        .is_none());
}
