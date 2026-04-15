//! Canonical command guidance used in user-facing hints and remediation text.

pub const CMD_CONFIG_SET_SECRET: &str = "aperture config secret set";
pub const CMD_CONFIG_LIST_SECRETS: &str = "aperture config secret list";
pub const CMD_CONFIG_REMOVE_SECRET: &str = "aperture config secret remove";
pub const CMD_CONFIG_CLEAR_SECRETS: &str = "aperture config secret clear";
pub const CMD_CONFIG_REINIT: &str = "aperture config api reinit";
pub const CMD_CONFIG_ADD: &str = "aperture config api add";
pub const CMD_CONFIG_SETTINGS: &str = "aperture config setting list";
pub const CMD_HELP_WITH_DESCRIBE_JSON: &str =
    "Check available operations with --help or --describe-json";
pub const CMD_HELP_WITH_DESCRIBE_JSON_COMMANDS: &str =
    "Check available commands with --help or --describe-json";

#[must_use]
pub fn secret_management_commands_hint() -> String {
    format!(
        "Use '{CMD_CONFIG_SET_SECRET} <api> <scheme> --env <VAR>', '{CMD_CONFIG_LIST_SECRETS} <api>', '{CMD_CONFIG_REMOVE_SECRET} <api> <scheme>', or '{CMD_CONFIG_CLEAR_SECRETS} <api> --force'."
    )
}

#[must_use]
pub fn auth_secret_management_hint(scheme_name: &str) -> String {
    format!(
        "Configure authentication for '{scheme_name}'. {}",
        secret_management_commands_hint()
    )
}

#[must_use]
pub fn auth_secret_missing_env_hint(env_var: &str) -> String {
    format!(
        "Set the {env_var} environment variable or run '{CMD_CONFIG_SET_SECRET} <api> <scheme> --env {env_var}' to configure the secret mapping."
    )
}

#[must_use]
pub fn cache_reinit_hint(spec_name: Option<&str>) -> String {
    spec_name.map_or_else(
        || format!("Run '{CMD_CONFIG_REINIT}' to regenerate the cache."),
        |name| format!("Run '{CMD_CONFIG_REINIT} {name}' to regenerate the cache."),
    )
}

#[must_use]
pub fn cached_spec_not_found_hint(spec_name: &str) -> String {
    format!("Run '{CMD_CONFIG_ADD} {spec_name} <spec-file>' first")
}
