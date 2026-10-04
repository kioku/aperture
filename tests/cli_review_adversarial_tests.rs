//! Regression sweeps for execution-default and concurrency parsers.
use aperture_cli::cli::Cli;
use aperture_cli::config::settings::{SettingKey, SettingValue};
use clap::Parser;

#[test]
fn invalid_execution_limits_are_rejected() {
    for flag in ["--batch-concurrency", "--timeout-secs"] {
        for value in [
            "",
            " ",
            "none",
            "false",
            "0",
            "-1",
            "18446744073709551615",
            "18446744073709551616",
        ] {
            assert!(
                Cli::try_parse_from(["aperture", "api", "test", flag, value]).is_err(),
                "{flag}={value}"
            );
        }
        assert!(Cli::try_parse_from(["aperture", "api", "test", flag, "1", flag, "2"]).is_err());
    }
    assert!(Cli::try_parse_from(["aperture", "api", "test", "--timeout-secs", "31536000"]).is_ok());
    assert!(
        Cli::try_parse_from(["aperture", "api", "test", "--timeout-secs", "31536001"]).is_err()
    );
}

#[test]
fn json_error_flag_rejects_invalid_and_duplicate_values() {
    for value in ["", " ", "none", "0", "1", "maybe", "TRUE"] {
        assert!(
            Cli::try_parse_from(["aperture", &format!("--json-errors={value}"), "overview"])
                .is_err()
        );
    }
    for value in ["true", "false"] {
        let cli = Cli::try_parse_from(["aperture", &format!("--json-errors={value}"), "overview"])
            .unwrap();
        assert_eq!(cli.json_errors, value == "true");
    }
    assert!(Cli::try_parse_from([
        "aperture",
        "--json-errors=true",
        "--json-errors=false",
        "overview"
    ])
    .is_err());
    assert!(Cli::try_parse_from(["aperture", "overview", "--unknown"]).is_err());
}

#[test]
fn configuration_defaults_reject_malformed_values() {
    for value in ["", " ", "none", "false", "0", "-1", "18446744073709551615"] {
        assert!(SettingValue::parse_for_key(SettingKey::DefaultTimeoutSecs, value).is_err());
    }
    for value in ["", " ", "none", "unknown"] {
        assert!(SettingValue::parse_for_key(SettingKey::AgentDefaultsJsonErrors, value).is_err());
    }
    assert!("unknown".parse::<SettingKey>().is_err());
    assert_eq!(
        SettingValue::parse_for_key(SettingKey::DefaultTimeoutSecs, "30").unwrap(),
        SettingValue::U64(30)
    );
    assert_eq!(
        SettingValue::parse_for_key(SettingKey::AgentDefaultsJsonErrors, "false").unwrap(),
        SettingValue::Bool(false)
    );
}
