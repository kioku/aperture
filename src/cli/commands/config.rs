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
                eprintln!("  {spec_name}: {e}");
                continue;
            }
        };
        match reinit_spec(manager, &validated, output) {
            Ok(()) => output.info(format!("  {spec_name}")),
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
