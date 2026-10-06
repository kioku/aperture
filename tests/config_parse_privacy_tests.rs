use aperture_cli::config::manager::ConfigManager;
use aperture_cli::fs::OsFileSystem;

const MARKER: &str = "synthetic-config-private";

fn malformed_configs() -> Vec<String> {
    vec![
        format!("[proxy]\nhttp = \"http://user:{MARKER}@localhost:9\n"),
        format!("[proxy]\nhttp = {{ url = \"http://user:{MARKER}@localhost:9\" }}\n"),
        format!("[proxy]\nhttps = {{ url = \"https://localhost/{MARKER}?token={MARKER}\" }}\n"),
        format!("[proxy]\nno_proxy = \"{MARKER}\"\n"),
        format!("[proxy]\nusername = [\"{MARKER}\"]\n"),
        format!("[agent_defaults]\njson_errors = \"{MARKER}\"\n"),
        format!("default_timeout_secs = \"é-{MARKER}\"\n"),
        format!("max_response_bytes = \"{MARKER}\"\n"),
        format!("[skills]\ndirectory = [\"{MARKER}\"]\n"),
        format!("[api_configs.audit.fetch_auth]\nmethod = '{MARKER}'\nenv_var = 'TEST_ONLY'\norigin = 'https://localhost'\n"),
        format!("default_timeout_secs = 30\ndefault_timeout_secs = \"{MARKER}\"\n"),
    ]
}

#[test]
fn configuration_parse_errors_never_retain_source_or_parser_values() {
    let directory = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, directory.path().into());
    for content in malformed_configs() {
        std::fs::write(directory.path().join("config.toml"), content).unwrap();
        let error = manager.load_global_config().unwrap_err();
        let diagnostic = format!(
            "{error} {error:?} {}",
            serde_json::to_string(&error.to_json()).unwrap()
        );
        assert!(!diagnostic.contains(MARKER), "{diagnostic}");
        assert!(!diagnostic.contains("localhost"), "{diagnostic}");
        assert!(diagnostic.contains("TOML parse error"), "{diagnostic}");
        assert!(diagnostic.contains("byte range"), "{diagnostic}");
        assert!(diagnostic.contains("syntax and structure"), "{diagnostic}");
    }
}

#[test]
fn setting_editor_parse_errors_never_retain_source_or_write() {
    use aperture_cli::config::settings::{SettingKey, SettingValue};
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.toml");
    let manager = ConfigManager::with_fs(OsFileSystem, directory.path().into());
    for content in [
        format!("[proxy]\nhttp = \"http://user:{MARKER}@localhost:9\n"),
        format!("default_timeout_secs = 30\ndefault_timeout_secs = \"{MARKER}\"\n"),
    ] {
        std::fs::write(&path, &content).unwrap();
        let error = manager
            .set_setting(&SettingKey::DefaultTimeoutSecs, &SettingValue::U64(42))
            .unwrap_err();
        let diagnostic = format!(
            "{error} {error:?} {}",
            serde_json::to_string(&error.to_json()).unwrap()
        );
        assert!(!diagnostic.contains(MARKER), "{diagnostic}");
        assert!(diagnostic.contains("byte range"), "{diagnostic}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }
}

#[test]
fn valid_and_missing_configuration_keep_existing_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let manager = ConfigManager::with_fs(OsFileSystem, directory.path().into());
    assert_eq!(
        manager.load_global_config().unwrap().default_timeout_secs,
        30
    );
    for content in ["", " \n\t ", "# comment only\n"] {
        std::fs::write(directory.path().join("config.toml"), content).unwrap();
        assert_eq!(
            manager.load_global_config().unwrap().default_timeout_secs,
            30
        );
    }
    let content = format!(
        "default_timeout_secs = 42\nunknown = 'ignored'\n[proxy]\nhttp = 'http://user:{MARKER}@localhost:9'\nno_proxy = ['é.example']\n[skills]\ndirectory = 'unicode-é'\n"
    );
    std::fs::write(directory.path().join("config.toml"), content).unwrap();
    let config = manager.load_global_config().unwrap();
    assert_eq!(config.default_timeout_secs, 42);
    assert_eq!(
        config.proxy.http.unwrap(),
        format!("http://user:{MARKER}@localhost:9")
    );
    assert_eq!(config.proxy.no_proxy, ["é.example"]);
    assert_eq!(config.skills.directory, "unicode-é");
}

#[cfg(feature = "integration")]
#[test]
fn cli_text_and_json_configuration_errors_are_source_free() {
    let directory = tempfile::tempdir().unwrap();
    for content in malformed_configs() {
        std::fs::write(directory.path().join("config.toml"), content).unwrap();
        for json_errors in [false, true] {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_aperture"));
            command.env("APERTURE_CONFIG_DIR", directory.path());
            command.env_remove("APERTURE_LOG_FILE");
            command.env_remove("APERTURE_LOG_FORMAT");
            if json_errors {
                command.arg("--json-errors");
            }
            let output = command.args(["config", "api", "list"]).output().unwrap();
            assert!(!output.status.success());
            let diagnostic = String::from_utf8(output.stderr).unwrap();
            assert!(!diagnostic.contains(MARKER), "{diagnostic}");
            assert!(diagnostic.contains("byte range"), "{diagnostic}");
            if json_errors {
                let value: serde_json::Value = serde_json::from_str(&diagnostic).unwrap();
                assert_eq!(value["error_type"], "Validation");
            }
        }
    }
}
