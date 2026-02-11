use aperture_cli::config::context_name::ApiContextName;

// ---- Valid names ----

#[test]
fn test_valid_simple_name() {
    assert!(ApiContextName::new("myapi").is_ok());
}

#[test]
fn test_valid_with_hyphen() {
    assert!(ApiContextName::new("my-api").is_ok());
}

#[test]
fn test_valid_with_underscore() {
    assert!(ApiContextName::new("api_v2").is_ok());
}

#[test]
fn test_valid_with_dot() {
    assert!(ApiContextName::new("foo.bar").is_ok());
}

#[test]
fn test_valid_uppercase() {
    assert!(ApiContextName::new("API123").is_ok());
}

#[test]
fn test_valid_single_char_letter() {
    assert!(ApiContextName::new("a").is_ok());
}

#[test]
fn test_valid_single_char_digit() {
    assert!(ApiContextName::new("1").is_ok());
}

#[test]
fn test_valid_mixed_case() {
    assert!(ApiContextName::new("MyApi-v2.1_beta").is_ok());
}

#[test]
fn test_valid_max_length() {
    let name = "a".repeat(64);
    assert!(ApiContextName::new(&name).is_ok());
}

#[test]
fn test_valid_starts_with_digit() {
    assert!(ApiContextName::new("1api").is_ok());
}

// ---- Invalid names ----

#[test]
fn test_invalid_empty() {
    let err = ApiContextName::new("").unwrap_err();
    assert!(err.to_string().contains("cannot be empty"), "{err}");
}

#[test]
fn test_invalid_path_traversal_dotdot() {
    let err = ApiContextName::new("../foo").unwrap_err();
    assert!(err.to_string().contains("must start with"), "{err}");
}

#[test]
fn test_invalid_forward_slash() {
    let err = ApiContextName::new("foo/bar").unwrap_err();
    assert!(err.to_string().contains("invalid character '/'"), "{err}");
}

#[test]
fn test_invalid_backslash() {
    let err = ApiContextName::new("foo\\bar").unwrap_err();
    assert!(err.to_string().contains("invalid character '\\'"), "{err}");
}

#[test]
fn test_invalid_leading_dot() {
    let err = ApiContextName::new(".hidden").unwrap_err();
    assert!(err.to_string().contains("must start with"), "{err}");
}

#[test]
fn test_invalid_exceeds_max_length() {
    let name = "a".repeat(65);
    let err = ApiContextName::new(&name).unwrap_err();
    assert!(err.to_string().contains("exceeds maximum length"), "{err}");
}

#[test]
fn test_invalid_leading_hyphen() {
    let err = ApiContextName::new("-api").unwrap_err();
    assert!(err.to_string().contains("must start with"), "{err}");
}

#[test]
fn test_invalid_leading_underscore() {
    let err = ApiContextName::new("_api").unwrap_err();
    assert!(err.to_string().contains("must start with"), "{err}");
}

#[test]
fn test_invalid_space() {
    let err = ApiContextName::new("my api").unwrap_err();
    assert!(err.to_string().contains("invalid character ' '"), "{err}");
}

#[test]
fn test_invalid_unicode() {
    let err = ApiContextName::new("café").unwrap_err();
    assert!(err.to_string().contains("invalid character"), "{err}");
}

#[test]
fn test_invalid_control_char() {
    let err = ApiContextName::new("foo\tbar").unwrap_err();
    assert!(err.to_string().contains("invalid character"), "{err}");
}

#[test]
fn test_invalid_colon() {
    let err = ApiContextName::new("foo:bar").unwrap_err();
    assert!(err.to_string().contains("invalid character ':'"), "{err}");
}

#[test]
fn test_invalid_null_byte() {
    let err = ApiContextName::new("foo\0bar").unwrap_err();
    assert!(err.to_string().contains("invalid character"), "{err}");
}

// ---- Trait impls ----

#[test]
fn test_as_str() {
    let name = ApiContextName::new("myapi").unwrap();
    assert_eq!(name.as_str(), "myapi");
}

#[test]
fn test_display() {
    let name = ApiContextName::new("myapi").unwrap();
    assert_eq!(format!("{name}"), "myapi");
}

#[test]
fn test_deref() {
    let name = ApiContextName::new("myapi").unwrap();
    // Deref allows using &str methods directly
    assert!(name.starts_with("my"));
}

#[test]
fn test_as_ref_str() {
    let name = ApiContextName::new("myapi").unwrap();
    let s: &str = name.as_ref();
    assert_eq!(s, "myapi");
}

// ---- Error message quality ----

#[test]
fn test_error_includes_suggestion() {
    let err = ApiContextName::new("../evil").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Invalid API context name"),
        "Error should identify the issue: {msg}"
    );
}
