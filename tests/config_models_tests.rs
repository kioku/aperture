use aperture::config::models::GlobalConfig;

#[test]
fn test_global_config_deserialization() {
    let toml_str = r#"
        default_timeout_secs = 60

        [agent_defaults]
        json_errors = true
    "#;

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 60);
    assert_eq!(config.agent_defaults.json_errors, true);
}

#[test]
fn test_global_config_default_values() {
    let toml_str = r#""#;

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 30);
    assert_eq!(config.agent_defaults.json_errors, false);
}

#[test]
fn test_global_config_partial_deserialization() {
    let toml_str = r#"
        default_timeout_secs = 120
    "#;

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 120);
    assert_eq!(config.agent_defaults.json_errors, false);
}

#[test]
fn test_global_config_agent_defaults_only() {
    let toml_str = r#"
        [agent_defaults]
        json_errors = true
    "#;

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 30);
    assert_eq!(config.agent_defaults.json_errors, true);
}
