use anyhow::anyhow;
use aperture_cli::error::Error;

#[test]
fn test_io_error_display() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err = Error::Io(io_err);
    assert_eq!(format!("{err}"), "I/O error: file not found");
}

#[test]
fn test_yaml_error_display() {
    let yaml_err = serde_yaml::from_str::<serde_yaml::Value>("key: - value").unwrap_err();
    let err = Error::Yaml(yaml_err);
    assert!(format!("{err}").starts_with("YAML parsing error: "));
}

#[test]
fn test_json_error_display() {
    let json_err = serde_json::from_str::<serde_json::Value>("{\"key\": ").unwrap_err();
    let err = Error::Json(json_err);
    assert!(format!("{err}").starts_with("JSON parsing error: "));
}

#[test]
fn test_toml_error_display() {
    let toml_err = toml::from_str::<toml::Value>("key = ").unwrap_err();
    let err = Error::Toml(toml_err);
    assert!(format!("{err}").starts_with("TOML parsing error: "));
}

#[test]
fn test_config_error_display() {
    let err = Error::invalid_config("Invalid configuration value");
    assert_eq!(
        format!("{err}"),
        "Validation: Invalid configuration: Invalid configuration value"
    );
}

#[test]
fn test_validation_error_display() {
    let err = Error::validation_error("Schema mismatch");
    assert_eq!(
        format!("{err}"),
        "Validation: Validation error: Schema mismatch"
    );
}

#[test]
fn test_anyhow_error_display() {
    let anyhow_err = anyhow!("Something went wrong");
    let err = Error::Anyhow(anyhow_err);
    assert_eq!(format!("{err}"), "Something went wrong");
}
