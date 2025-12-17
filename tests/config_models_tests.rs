use aperture_cli::config::models::{ApertureSecret, GlobalConfig, SecretSource};

#[test]
fn test_global_config_deserialization() {
    let toml_str = r"
        default_timeout_secs = 60

        [agent_defaults]
        json_errors = true
    ";

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 60);
    assert!(config.agent_defaults.json_errors);
}

#[test]
fn test_global_config_default_values() {
    let toml_str = r"";

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 30);
    assert!(!config.agent_defaults.json_errors);
}

#[test]
fn test_global_config_partial_deserialization() {
    let toml_str = r"
        default_timeout_secs = 120
    ";

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 120);
    assert!(!config.agent_defaults.json_errors);
}

#[test]
fn test_global_config_agent_defaults_only() {
    let toml_str = r"
        [agent_defaults]
        json_errors = true
    ";

    let config: GlobalConfig = toml::from_str(toml_str).unwrap();

    assert_eq!(config.default_timeout_secs, 30);
    assert!(config.agent_defaults.json_errors);
}

#[test]
fn test_aperture_secret_deserialization() {
    let yaml_str = r"
        source: env
        name: MY_API_KEY
    ";

    let secret: ApertureSecret = serde_yaml::from_str(yaml_str).unwrap();

    assert_eq!(secret.source, SecretSource::Env);
    assert_eq!(secret.name, "MY_API_KEY");
}
