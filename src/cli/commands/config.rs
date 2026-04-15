//! Handlers for `aperture config *` subcommands.

use crate::config::context_name::ApiContextName;
use crate::config::manager::{get_config_dir, ConfigManager};
use crate::config::models::SecretSource;
use crate::constants;
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::output::Output;
use crate::response_cache::{CacheConfig, ResponseCache};
use std::path::PathBuf;

/// Validates and returns the API context name, returning an error for invalid names.
pub fn validate_api_name(name: &str) -> Result<ApiContextName, Error> {
    ApiContextName::new(name)
}

#[allow(clippy::needless_pass_by_value)]
async fn handle_add_spec_command(
    manager: &ConfigManager<OsFileSystem>,
    name: String,
    file_or_url: String,
    force: bool,
    strict: bool,
    output: &Output,
) -> Result<(), Error> {
    let name = validate_api_name(&name)?;
    manager
        .add_spec_auto(&name, &file_or_url, force, strict)
        .await?;
    output.success(format!("Spec '{name}' added successfully."));
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_remove_spec_command(
    manager: &ConfigManager<OsFileSystem>,
    name: String,
    output: &Output,
) -> Result<(), Error> {
    let name = validate_api_name(&name)?;
    manager.remove_spec(&name)?;
    output.success(format!("Spec '{name}' removed successfully."));
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_edit_spec_command(
    manager: &ConfigManager<OsFileSystem>,
    name: String,
    output: &Output,
) -> Result<(), Error> {
    let name = validate_api_name(&name)?;
    manager.edit_spec(&name)?;
    output.success(format!("Opened spec '{name}' in editor."));
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_get_url_command(
    manager: &ConfigManager<OsFileSystem>,
    name: String,
    output: &Output,
) -> Result<(), Error> {
    let name = validate_api_name(&name)?;
    let (base_override, env_urls, resolved) = manager.get_url(&name)?;
    print_url_configuration(
        &name,
        base_override.as_deref(),
        &env_urls,
        &resolved,
        output,
    );
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_remove_secret_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    scheme_name: String,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    manager.remove_secret(&api_name, &scheme_name)?;
    output.success(format!(
        "Removed secret configuration for scheme '{scheme_name}' from API '{api_name}'"
    ));
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_set_setting_command(
    manager: &ConfigManager<OsFileSystem>,
    key: String,
    value: String,
    output: &Output,
) -> Result<(), Error> {
    use crate::config::settings::{SettingKey, SettingValue};

    let setting_key: SettingKey = key.parse()?;
    let setting_value = SettingValue::parse_for_key(setting_key, &value)?;
    manager.set_setting(&setting_key, &setting_value)?;
    output.success(format!("Set {key} = {value}"));
    Ok(())
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn handle_set_mapping_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    group: Option<Vec<String>>,
    operation: Option<String>,
    name: Option<String>,
    op_group: Option<String>,
    alias: Option<String>,
    remove_alias: Option<String>,
    hidden: bool,
    visible: bool,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    handle_set_mapping(
        manager,
        &api_name,
        group.as_deref(),
        operation.as_deref(),
        name.as_deref(),
        op_group.as_deref(),
        alias.as_deref(),
        remove_alias.as_deref(),
        hidden,
        visible,
        output,
    )
}

#[allow(clippy::needless_pass_by_value)]
fn handle_list_mappings_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    handle_list_mappings(manager, &api_name, output)
}

#[allow(clippy::needless_pass_by_value)]
fn handle_remove_mapping_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    group: Option<String>,
    operation: Option<String>,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    handle_remove_mapping(manager, &api_name, group, operation, output)
}

fn handle_list_specs(
    manager: &ConfigManager<OsFileSystem>,
    verbose: bool,
    output: &Output,
) -> Result<(), Error> {
    let specs = manager.list_specs()?;
    if specs.is_empty() {
        output.info("No API specifications found.");
    } else {
        output.info("Registered API specifications:");
        list_specs_with_details(manager, specs, verbose, output);
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_set_url(
    manager: &ConfigManager<OsFileSystem>,
    name: String,
    url: String,
    env: Option<String>,
    output: &Output,
) -> Result<(), Error> {
    let name = validate_api_name(&name)?;
    manager.set_url(&name, &url, env.as_deref())?;
    if let Some(environment) = env {
        output.success(format!(
            "Set base URL for '{name}' in environment '{environment}': {url}"
        ));
    } else {
        output.success(format!("Set base URL for '{name}': {url}"));
    }
    Ok(())
}

fn handle_list_urls(manager: &ConfigManager<OsFileSystem>, output: &Output) -> Result<(), Error> {
    let all_urls = manager.list_urls()?;
    if all_urls.is_empty() {
        output.info("No base URLs configured.");
        return Ok(());
    }
    output.info("Configured base URLs:");
    for (api_name, (base_override, env_urls)) in all_urls {
        print_api_url_entry(&api_name, base_override.as_deref(), &env_urls, output);
    }
    Ok(())
}

fn handle_reinit(
    manager: &ConfigManager<OsFileSystem>,
    context: Option<String>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    if all {
        reinit_all_specs(manager, output)?;
        return Ok(());
    }
    let Some(spec_name) = context else {
        // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
        // ast-grep-ignore: no-println
        eprintln!("Error: Either specify a spec name or use --all flag");
        std::process::exit(1);
    };
    let spec_name = validate_api_name(&spec_name)?;
    reinit_spec(manager, &spec_name, output)
}

#[allow(clippy::needless_pass_by_value)]
async fn handle_clear_cache(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<String>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    if let Some(ref name) = api_name {
        validate_api_name(name)?;
    }
    clear_response_cache(manager, api_name.as_deref(), all, output).await
}

#[allow(clippy::needless_pass_by_value)]
async fn handle_cache_stats(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<String>,
    output: &Output,
) -> Result<(), Error> {
    if let Some(ref name) = api_name {
        validate_api_name(name)?;
    }
    show_cache_stats(manager, api_name.as_deref(), output).await
}

#[allow(clippy::needless_pass_by_value)]
fn handle_set_secret(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    scheme_name: Option<String>,
    env: Option<String>,
    interactive: bool,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    if interactive {
        manager.set_secret_interactive(&api_name)?;
        return Ok(());
    }
    let (Some(scheme), Some(env_var)) = (scheme_name, env) else {
        return Err(Error::invalid_config(
            "Either provide --scheme and --env, or use --interactive",
        ));
    };
    manager.set_secret(&api_name, &scheme, &env_var)?;
    output.success(format!(
        "Set secret for scheme '{scheme}' in API '{api_name}' to use environment variable '{env_var}'"
    ));
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_list_secrets(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    let secrets = manager.list_secrets(&api_name)?;
    if secrets.is_empty() {
        output.info(format!("No secrets configured for API '{api_name}'"));
    } else {
        print_secrets_list(&api_name, secrets, output);
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_clear_secrets(
    manager: &ConfigManager<OsFileSystem>,
    api_name: String,
    force: bool,
    output: &Output,
) -> Result<(), Error> {
    let api_name = validate_api_name(&api_name)?;
    let secrets = manager.list_secrets(&api_name)?;
    if secrets.is_empty() {
        output.info(format!("No secrets configured for API '{api_name}'"));
        return Ok(());
    }

    if force {
        return clear_all_api_secrets(manager, &api_name, output);
    }

    announce_secret_clear(&api_name, &secrets, output);
    if !crate::interactive::confirm("Are you sure you want to continue?")? {
        output.info("Operation cancelled");
        return Ok(());
    }

    clear_all_api_secrets(manager, &api_name, output)
}

fn announce_secret_clear(
    api_name: &ApiContextName,
    secrets: &std::collections::HashMap<String, crate::config::models::ApertureSecret>,
    output: &Output,
) {
    output.info(format!(
        "This will remove all {} secret configuration(s) for API '{api_name}':",
        secrets.len()
    ));
    for scheme_name in secrets.keys() {
        output.info(format!("  - {scheme_name}"));
    }
}

fn clear_all_api_secrets(
    manager: &ConfigManager<OsFileSystem>,
    api_name: &ApiContextName,
    output: &Output,
) -> Result<(), Error> {
    manager.clear_secrets(api_name)?;
    output.success(format!(
        "Cleared all secret configurations for API '{api_name}'"
    ));
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn handle_get_setting(
    manager: &ConfigManager<OsFileSystem>,
    key: String,
    json: bool,
) -> Result<(), Error> {
    use crate::config::settings::SettingKey;

    let setting_key: SettingKey = key.parse()?;
    let value = manager.get_setting(&setting_key)?;
    if json {
        // ast-grep-ignore: no-println
        println!(
            "{}",
            serde_json::json!({ "key": key, "value": value.to_string() })
        );
    } else {
        // ast-grep-ignore: no-println
        println!("{value}");
    }
    Ok(())
}

fn handle_settings(
    manager: &ConfigManager<OsFileSystem>,
    json: bool,
    output: &Output,
) -> Result<(), Error> {
    let settings = manager.list_settings()?;
    print_settings_list(settings, json, output)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigCommandFamily {
    Specs,
    Cache,
    Secrets,
    Settings,
    Mappings,
}

fn normalize_api_config_command(
    command: crate::cli::ConfigApiCommands,
) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigApiCommands::Add {
            name,
            file_or_url,
            force,
            strict,
        } => crate::cli::ConfigCommands::Add {
            name,
            file_or_url,
            force,
            strict,
        },
        crate::cli::ConfigApiCommands::List { verbose } => {
            crate::cli::ConfigCommands::List { verbose }
        }
        crate::cli::ConfigApiCommands::Remove { name } => {
            crate::cli::ConfigCommands::Remove { name }
        }
        crate::cli::ConfigApiCommands::Edit { name } => crate::cli::ConfigCommands::Edit { name },
        crate::cli::ConfigApiCommands::Reinit { context, all } => {
            crate::cli::ConfigCommands::Reinit { context, all }
        }
    }
}

fn normalize_url_config_command(
    command: crate::cli::ConfigUrlCommands,
) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigUrlCommands::Set { name, url, env } => {
            crate::cli::ConfigCommands::SetUrl { name, url, env }
        }
        crate::cli::ConfigUrlCommands::Get { name } => crate::cli::ConfigCommands::GetUrl { name },
        crate::cli::ConfigUrlCommands::List => crate::cli::ConfigCommands::ListUrls {},
    }
}

fn normalize_secret_config_command(
    command: crate::cli::ConfigSecretCommands,
) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigSecretCommands::Set {
            api_name,
            scheme_name,
            env,
            interactive,
        } => crate::cli::ConfigCommands::SetSecret {
            api_name,
            scheme_name,
            env,
            interactive,
        },
        crate::cli::ConfigSecretCommands::List { api_name } => {
            crate::cli::ConfigCommands::ListSecrets { api_name }
        }
        crate::cli::ConfigSecretCommands::Remove {
            api_name,
            scheme_name,
        } => crate::cli::ConfigCommands::RemoveSecret {
            api_name,
            scheme_name,
        },
        crate::cli::ConfigSecretCommands::Clear { api_name, force } => {
            crate::cli::ConfigCommands::ClearSecrets { api_name, force }
        }
    }
}

fn normalize_cache_config_command(
    command: crate::cli::ConfigCacheCommands,
) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigCacheCommands::Clear { api_name, all } => {
            crate::cli::ConfigCommands::ClearCache { api_name, all }
        }
        crate::cli::ConfigCacheCommands::Stats { api_name } => {
            crate::cli::ConfigCommands::CacheStats { api_name }
        }
    }
}

fn normalize_setting_config_command(
    command: crate::cli::ConfigSettingCommands,
) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigSettingCommands::Set { key, value } => {
            crate::cli::ConfigCommands::Set { key, value }
        }
        crate::cli::ConfigSettingCommands::Get { key, json } => {
            crate::cli::ConfigCommands::Get { key, json }
        }
        crate::cli::ConfigSettingCommands::List { json } => {
            crate::cli::ConfigCommands::Settings { json }
        }
    }
}

fn normalize_mapping_config_command(
    command: crate::cli::ConfigMappingCommands,
) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigMappingCommands::Set {
            api_name,
            group,
            operation,
            name,
            op_group,
            alias,
            remove_alias,
            hidden,
            visible,
        } => crate::cli::ConfigCommands::SetMapping {
            api_name,
            group,
            operation,
            name,
            op_group,
            alias,
            remove_alias,
            hidden,
            visible,
        },
        crate::cli::ConfigMappingCommands::List { api_name } => {
            crate::cli::ConfigCommands::ListMappings { api_name }
        }
        crate::cli::ConfigMappingCommands::Remove {
            api_name,
            group,
            operation,
        } => crate::cli::ConfigCommands::RemoveMapping {
            api_name,
            group,
            operation,
        },
    }
}

fn normalize_config_command(command: crate::cli::ConfigCommands) -> crate::cli::ConfigCommands {
    match command {
        crate::cli::ConfigCommands::Api { command } => normalize_api_config_command(command),
        crate::cli::ConfigCommands::Url { command } => normalize_url_config_command(command),
        crate::cli::ConfigCommands::Secret { command } => normalize_secret_config_command(command),
        crate::cli::ConfigCommands::Cache { command } => normalize_cache_config_command(command),
        crate::cli::ConfigCommands::Setting { command } => {
            normalize_setting_config_command(command)
        }
        crate::cli::ConfigCommands::Mapping { command } => {
            normalize_mapping_config_command(command)
        }
        legacy => legacy,
    }
}

const fn config_command_family(command: &crate::cli::ConfigCommands) -> ConfigCommandFamily {
    match command {
        crate::cli::ConfigCommands::Api { .. }
        | crate::cli::ConfigCommands::Url { .. }
        | crate::cli::ConfigCommands::Add { .. }
        | crate::cli::ConfigCommands::List { .. }
        | crate::cli::ConfigCommands::Remove { .. }
        | crate::cli::ConfigCommands::Edit { .. }
        | crate::cli::ConfigCommands::SetUrl { .. }
        | crate::cli::ConfigCommands::GetUrl { .. }
        | crate::cli::ConfigCommands::ListUrls {}
        | crate::cli::ConfigCommands::Reinit { .. } => ConfigCommandFamily::Specs,
        crate::cli::ConfigCommands::Cache { .. }
        | crate::cli::ConfigCommands::ClearCache { .. }
        | crate::cli::ConfigCommands::CacheStats { .. } => ConfigCommandFamily::Cache,
        crate::cli::ConfigCommands::Secret { .. }
        | crate::cli::ConfigCommands::SetSecret { .. }
        | crate::cli::ConfigCommands::ListSecrets { .. }
        | crate::cli::ConfigCommands::RemoveSecret { .. }
        | crate::cli::ConfigCommands::ClearSecrets { .. } => ConfigCommandFamily::Secrets,
        crate::cli::ConfigCommands::Setting { .. }
        | crate::cli::ConfigCommands::Set { .. }
        | crate::cli::ConfigCommands::Get { .. }
        | crate::cli::ConfigCommands::Settings { .. } => ConfigCommandFamily::Settings,
        crate::cli::ConfigCommands::Mapping { .. }
        | crate::cli::ConfigCommands::SetMapping { .. }
        | crate::cli::ConfigCommands::ListMappings { .. }
        | crate::cli::ConfigCommands::RemoveMapping { .. } => ConfigCommandFamily::Mappings,
    }
}

async fn execute_specs_config_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::Add { .. }
        | crate::cli::ConfigCommands::List { .. }
        | crate::cli::ConfigCommands::Remove { .. }
        | crate::cli::ConfigCommands::Edit { .. } => {
            execute_specs_catalog_command(manager, command, output).await
        }
        crate::cli::ConfigCommands::SetUrl { .. }
        | crate::cli::ConfigCommands::GetUrl { .. }
        | crate::cli::ConfigCommands::ListUrls {} => {
            execute_specs_url_command(manager, command, output)
        }
        crate::cli::ConfigCommands::Reinit { context, all } => {
            handle_reinit(manager, context, all, output)
        }
        _ => unreachable!("command family routing must be exhaustive"),
    }
}

async fn execute_specs_catalog_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::Add {
            name,
            file_or_url,
            force,
            strict,
        } => handle_add_spec_command(manager, name, file_or_url, force, strict, output).await,
        crate::cli::ConfigCommands::List { verbose } => handle_list_specs(manager, verbose, output),
        crate::cli::ConfigCommands::Remove { name } => {
            handle_remove_spec_command(manager, name, output)
        }
        crate::cli::ConfigCommands::Edit { name } => {
            handle_edit_spec_command(manager, name, output)
        }
        _ => unreachable!("catalog command routing must be exhaustive"),
    }
}

fn execute_specs_url_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::SetUrl { name, url, env } => {
            handle_set_url(manager, name, url, env, output)
        }
        crate::cli::ConfigCommands::GetUrl { name } => {
            handle_get_url_command(manager, name, output)
        }
        crate::cli::ConfigCommands::ListUrls {} => handle_list_urls(manager, output),
        _ => unreachable!("url command routing must be exhaustive"),
    }
}

async fn execute_cache_config_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::ClearCache { api_name, all } => {
            handle_clear_cache(manager, api_name, all, output).await
        }
        crate::cli::ConfigCommands::CacheStats { api_name } => {
            handle_cache_stats(manager, api_name, output).await
        }
        _ => unreachable!("command family routing must be exhaustive"),
    }
}

fn execute_secret_config_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::SetSecret {
            api_name,
            scheme_name,
            env,
            interactive,
        } => handle_set_secret(manager, api_name, scheme_name, env, interactive, output),
        crate::cli::ConfigCommands::ListSecrets { api_name } => {
            handle_list_secrets(manager, api_name, output)
        }
        crate::cli::ConfigCommands::RemoveSecret {
            api_name,
            scheme_name,
        } => handle_remove_secret_command(manager, api_name, scheme_name, output),
        crate::cli::ConfigCommands::ClearSecrets { api_name, force } => {
            handle_clear_secrets(manager, api_name, force, output)
        }
        _ => unreachable!("command family routing must be exhaustive"),
    }
}

fn execute_settings_config_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::Set { key, value } => {
            handle_set_setting_command(manager, key, value, output)
        }
        crate::cli::ConfigCommands::Get { key, json } => handle_get_setting(manager, key, json),
        crate::cli::ConfigCommands::Settings { json } => handle_settings(manager, json, output),
        _ => unreachable!("command family routing must be exhaustive"),
    }
}

fn execute_mapping_config_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    match command {
        crate::cli::ConfigCommands::SetMapping {
            api_name,
            group,
            operation,
            name,
            op_group,
            alias,
            remove_alias,
            hidden,
            visible,
        } => handle_set_mapping_command(
            manager,
            api_name,
            group,
            operation,
            name,
            op_group,
            alias,
            remove_alias,
            hidden,
            visible,
            output,
        ),
        crate::cli::ConfigCommands::ListMappings { api_name } => {
            handle_list_mappings_command(manager, api_name, output)
        }
        crate::cli::ConfigCommands::RemoveMapping {
            api_name,
            group,
            operation,
        } => handle_remove_mapping_command(manager, api_name, group, operation, output),
        _ => unreachable!("command family routing must be exhaustive"),
    }
}

/// Execute `aperture config <subcommand>`.
#[allow(clippy::too_many_lines)]
pub async fn execute_config_command(
    manager: &ConfigManager<OsFileSystem>,
    command: crate::cli::ConfigCommands,
    output: &Output,
) -> Result<(), Error> {
    let command = normalize_config_command(command);
    match config_command_family(&command) {
        ConfigCommandFamily::Specs => execute_specs_config_command(manager, command, output).await,
        ConfigCommandFamily::Cache => execute_cache_config_command(manager, command, output).await,
        ConfigCommandFamily::Secrets => execute_secret_config_command(manager, command, output),
        ConfigCommandFamily::Settings => execute_settings_config_command(manager, command, output),
        ConfigCommandFamily::Mappings => execute_mapping_config_command(manager, command, output),
    }
}

/// Print the list of configured secrets for an API
pub fn print_secrets_list(
    api_name: &str,
    secrets: std::collections::HashMap<String, crate::config::models::ApertureSecret>,
    output: &Output,
) {
    output.info(format!("Configured secrets for API '{api_name}':"));
    for (scheme_name, secret) in secrets {
        match secret.source {
            SecretSource::Env => {
                // ast-grep-ignore: no-println
                println!("  {scheme_name}: environment variable '{}'", secret.name);
            }
        }
    }
}

/// Print a single API URL entry in the list
pub fn print_api_url_entry(
    api_name: &str,
    base_override: Option<&str>,
    env_urls: &std::collections::HashMap<String, String>,
    output: &Output,
) {
    // ast-grep-ignore: no-println
    println!("\n{api_name}:");
    if let Some(base) = base_override {
        // ast-grep-ignore: no-println
        println!("  Base override: {base}");
    }
    if !env_urls.is_empty() {
        output.info("  Environment URLs:");
        for (env, url) in env_urls {
            // ast-grep-ignore: no-println
            println!("    {env}: {url}");
        }
    }
}

/// Print URL configuration for a specific API
pub fn print_url_configuration(
    name: &str,
    base_override: Option<&str>,
    env_urls: &std::collections::HashMap<String, String>,
    resolved: &str,
    output: &Output,
) {
    output.info(format!("Base URL configuration for '{name}':"));
    if let Some(base) = base_override {
        // ast-grep-ignore: no-println
        println!("  Base override: {base}");
    } else {
        // ast-grep-ignore: no-println
        println!("  Base override: (none)");
    }
    if !env_urls.is_empty() {
        // ast-grep-ignore: no-println
        println!("  Environment URLs:");
        for (env, url) in env_urls {
            // ast-grep-ignore: no-println
            println!("    {env}: {url}");
        }
    }
    // ast-grep-ignore: no-println
    println!("\nResolved URL (current): {resolved}");
    if let Ok(current_env) = std::env::var(constants::ENV_APERTURE_ENV) {
        output.info(format!("(Using APERTURE_ENV={current_env})"));
    }
}

pub fn reinit_spec(
    manager: &ConfigManager<OsFileSystem>,
    spec_name: &ApiContextName,
    output: &Output,
) -> Result<(), Error> {
    output.info(format!("Reinitializing cached specification: {spec_name}"));
    let specs = manager.list_specs()?;
    if !specs.contains(&spec_name.to_string()) {
        return Err(Error::spec_not_found(spec_name.as_str()));
    }
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let specs_dir = config_dir.join(constants::DIR_SPECS);
    let spec_path = specs_dir.join(format!("{spec_name}.yaml"));
    let strict = manager.get_strict_preference(spec_name).unwrap_or(false);
    manager.add_spec(spec_name, &spec_path, true, strict)?;
    output.success(format!(
        "Successfully reinitialized cache for '{spec_name}'"
    ));
    Ok(())
}

pub fn reinit_all_specs(
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    let specs = manager.list_specs()?;
    if specs.is_empty() {
        output.info("No API specifications found to reinitialize.");
        return Ok(());
    }
    output.info(format!(
        "Reinitializing {} cached specification(s)...",
        specs.len()
    ));
    for spec_name in &specs {
        let validated = match validate_api_name(spec_name) {
            Ok(v) => v,
            Err(e) => {
                // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
                // ast-grep-ignore: no-println
                eprintln!("  {spec_name}: {e}");
                continue;
            }
        };
        match reinit_spec(manager, &validated, output) {
            Ok(()) => output.info(format!("  {spec_name}")),
            // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
            // ast-grep-ignore: no-println
            Err(e) => eprintln!("  {spec_name}: {e}"),
        }
    }
    output.success("Reinitialization complete.");
    Ok(())
}

pub fn list_specs_with_details(
    manager: &ConfigManager<OsFileSystem>,
    specs: Vec<String>,
    verbose: bool,
    output: &Output,
) {
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    for spec_name in specs {
        if !verbose {
            // ast-grep-ignore: no-println
            println!("- {spec_name}");
            continue;
        }
        let Ok(cached_spec) = crate::engine::loader::load_cached_spec(&cache_dir, &spec_name)
        else {
            // ast-grep-ignore: no-println
            println!("- {spec_name}");
            continue;
        };
        // ast-grep-ignore: no-println
        println!("- {spec_name}:");
        output.info(format!("  Version: {}", cached_spec.version));
        let available = cached_spec.commands.len();
        let skipped = cached_spec.skipped_endpoints.len();
        let total = available + skipped;
        if skipped > 0 {
            output.info(format!(
                "  Endpoints: {available} of {total} available ({skipped} skipped)"
            ));
            display_skipped_endpoints_info(&cached_spec, output);
        } else {
            output.info(format!("  Endpoints: {available} available"));
        }
    }
}

fn display_skipped_endpoints_info(cached_spec: &crate::cache::models::CachedSpec, output: &Output) {
    output.info("  Skipped endpoints:");
    for endpoint in &cached_spec.skipped_endpoints {
        output.info(format!(
            "    - {} {} - {} not supported",
            endpoint.method, endpoint.path, endpoint.content_type
        ));
    }
}

pub fn print_settings_list(
    settings: Vec<crate::config::settings::SettingInfo>,
    json: bool,
    output: &Output,
) -> Result<(), Error> {
    if json {
        // ast-grep-ignore: no-println
        println!("{}", serde_json::to_string_pretty(&settings)?);
        return Ok(());
    }
    output.info("Available configuration settings:");
    // ast-grep-ignore: no-println
    println!();
    for setting in settings {
        // ast-grep-ignore: no-println
        println!("  {} = {}", setting.key, setting.value);
        // ast-grep-ignore: no-println
        println!(
            "    Type: {}  Default: {}",
            setting.type_name, setting.default
        );
        // ast-grep-ignore: no-println
        println!("    {}", setting.description);
        // ast-grep-ignore: no-println
        println!();
    }
    Ok(())
}

/// Clear response cache for a specific API or all APIs
pub async fn clear_response_cache(
    _manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_config = CacheConfig {
        cache_dir: config_dir
            .join(constants::DIR_CACHE)
            .join(constants::DIR_RESPONSES),
        ..Default::default()
    };
    let cache = ResponseCache::new(cache_config)?;
    let cleared_count = if all {
        cache.clear_all().await?
    } else {
        let Some(api) = api_name else {
            // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
            // ast-grep-ignore: no-println
            eprintln!("Error: Either specify an API name or use --all flag");
            std::process::exit(1);
        };
        cache.clear_api_cache(api).await?
    };
    if all {
        output.success(format!(
            "Cleared {cleared_count} cached responses for all APIs"
        ));
    } else {
        let Some(api) = api_name else {
            unreachable!("API name must be Some if not all");
        };
        output.success(format!(
            "Cleared {cleared_count} cached responses for API '{api}'"
        ));
    }
    Ok(())
}

/// Show cache statistics for a specific API or all APIs
pub async fn show_cache_stats(
    _manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    output: &Output,
) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_config = CacheConfig {
        cache_dir: config_dir
            .join(constants::DIR_CACHE)
            .join(constants::DIR_RESPONSES),
        ..Default::default()
    };
    let cache = ResponseCache::new(cache_config)?;
    let stats = cache.get_stats(api_name).await?;
    if let Some(api) = api_name {
        output.info(format!("Cache statistics for API '{api}':"));
    } else {
        output.info("Cache statistics for all APIs:");
    }
    // ast-grep-ignore: no-println
    println!("  Total entries: {}", stats.total_entries);
    // ast-grep-ignore: no-println
    println!("  Valid entries: {}", stats.valid_entries);
    // ast-grep-ignore: no-println
    println!("  Expired entries: {}", stats.expired_entries);
    #[allow(clippy::cast_precision_loss)]
    let size_mb = stats.total_size_bytes as f64 / 1024.0 / 1024.0;
    // ast-grep-ignore: no-println
    println!("  Total size: {size_mb:.2} MB");
    if stats.total_entries != 0 {
        #[allow(clippy::cast_precision_loss)]
        let hit_rate = stats.valid_entries as f64 / stats.total_entries as f64 * 100.0;
        // ast-grep-ignore: no-println
        println!("  Hit rate: {hit_rate:.1}%");
    }
    Ok(())
}

// ── Command Mapping Handlers ──

const fn resolve_hidden_flag(hidden: bool, visible: bool) -> Option<bool> {
    match (hidden, visible) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    }
}

fn describe_mapping_changes(
    name: Option<&str>,
    op_group: Option<&str>,
    alias: Option<&str>,
    remove_alias: Option<&str>,
    hidden: bool,
    visible: bool,
) -> String {
    let mut changes = Vec::new();

    if let Some(n) = name {
        changes.push(format!("name='{n}'"));
    }
    if let Some(g) = op_group {
        changes.push(format!("group='{g}'"));
    }
    if let Some(a) = alias {
        changes.push(format!("alias+='{a}'"));
    }
    if let Some(a) = remove_alias {
        changes.push(format!("alias-='{a}'"));
    }
    if hidden {
        changes.push("hidden=true".to_string());
    }
    if visible {
        changes.push("hidden=false".to_string());
    }

    if changes.is_empty() {
        "(no changes)".to_string()
    } else {
        changes.join(", ")
    }
}

/// Handle the `config set-mapping` command
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
pub fn handle_set_mapping(
    manager: &ConfigManager<OsFileSystem>,
    api_name: &crate::config::context_name::ApiContextName,
    group: Option<&[String]>,
    operation: Option<&str>,
    name: Option<&str>,
    op_group: Option<&str>,
    alias: Option<&str>,
    remove_alias: Option<&str>,
    hidden: bool,
    visible: bool,
    output: &Output,
) -> Result<(), Error> {
    if let Some([original, new_name, ..]) = group {
        manager.set_group_mapping(api_name, original, new_name)?;
        output.success(format!(
            "Set group mapping for '{api_name}': '{original}' → '{new_name}'"
        ));
        output.info("Run 'aperture config reinit' to apply changes.");
        return Ok(());
    }

    let Some(op_id) = operation else {
        return Err(Error::invalid_config(
            "Either --group or --operation must be specified",
        ));
    };

    let hidden_flag = resolve_hidden_flag(hidden, visible);
    manager.set_operation_mapping(api_name, op_id, name, op_group, alias, hidden_flag)?;

    if let Some(alias_to_remove) = remove_alias {
        manager.remove_alias(api_name, op_id, alias_to_remove)?;
    }

    output.success(format!(
        "Set operation mapping for '{api_name}': '{op_id}' → {}",
        describe_mapping_changes(name, op_group, alias, remove_alias, hidden, visible)
    ));
    output.info("Run 'aperture config reinit' to apply changes.");
    Ok(())
}

/// Handle the `config list-mappings` command
pub fn handle_list_mappings(
    manager: &ConfigManager<OsFileSystem>,
    api_name: &crate::config::context_name::ApiContextName,
    output: &Output,
) -> Result<(), Error> {
    let mapping = manager.get_command_mapping(api_name)?;
    let Some(mapping) = mapping else {
        output.info(format!(
            "No command mappings configured for API '{api_name}'"
        ));
        return Ok(());
    };

    if mapping.groups.is_empty() && mapping.operations.is_empty() {
        output.info(format!(
            "No command mappings configured for API '{api_name}'"
        ));
        return Ok(());
    }

    output.info(format!("Command mappings for API '{api_name}':"));

    if !mapping.groups.is_empty() {
        // ast-grep-ignore: no-println
        println!("\n  Group renames:");
        for (original, new_name) in &mapping.groups {
            // ast-grep-ignore: no-println
            println!("    '{original}' → '{new_name}'");
        }
    }

    if !mapping.operations.is_empty() {
        // ast-grep-ignore: no-println
        println!("\n  Operation mappings:");
        for (op_id, op_mapping) in &mapping.operations {
            print_operation_mapping(op_id, op_mapping);
        }
    }

    Ok(())
}

/// Handle the `config remove-mapping` command
pub fn handle_remove_mapping(
    manager: &ConfigManager<OsFileSystem>,
    api_name: &crate::config::context_name::ApiContextName,
    group: Option<String>,
    operation: Option<String>,
    output: &Output,
) -> Result<(), Error> {
    match (group, operation) {
        (Some(ref original), None) => {
            manager.remove_group_mapping(api_name, original)?;
            output.success(format!(
                "Removed group mapping for tag '{original}' from API '{api_name}'"
            ));
        }
        (None, Some(ref op_id)) => {
            manager.remove_operation_mapping(api_name, op_id)?;
            output.success(format!(
                "Removed operation mapping for '{op_id}' from API '{api_name}'"
            ));
        }
        (Some(_), Some(_)) => {
            return Err(Error::invalid_config(
                "Specify either --group or --operation, not both",
            ));
        }
        (None, None) => {
            return Err(Error::invalid_config(
                "Either --group or --operation must be specified",
            ));
        }
    }
    output.info("Run 'aperture config reinit' to apply changes.");
    Ok(())
}

/// Prints details of a single operation mapping
fn print_operation_mapping(op_id: &str, op_mapping: &crate::config::models::OperationMapping) {
    // ast-grep-ignore: no-println
    println!("    {op_id}:");
    if let Some(ref name) = op_mapping.name {
        // ast-grep-ignore: no-println
        println!("      name: {name}");
    }
    if let Some(ref group) = op_mapping.group {
        // ast-grep-ignore: no-println
        println!("      group: {group}");
    }
    if !op_mapping.aliases.is_empty() {
        // ast-grep-ignore: no-println
        println!("      aliases: {}", op_mapping.aliases.join(", "));
    }
    if op_mapping.hidden {
        // ast-grep-ignore: no-println
        println!("      hidden: true");
    }
}
