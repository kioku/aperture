use aperture_cli::cli::commands::config::validate_api_name;
use aperture_cli::cli::{Cli, Commands, ConfigCommands};
use aperture_cli::config::manager::ConfigManager;
use aperture_cli::constants;
use aperture_cli::error::Error;
use aperture_cli::fs::OsFileSystem;
use aperture_cli::interactive::confirm;
use aperture_cli::output::Output;
use clap::Parser;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    aperture_cli::cli::tracing_init::init_tracing(cli.verbosity);
    let json_errors = cli.json_errors;
    let output = Output::new(cli.quiet, cli.json_errors);

    let manager = std::env::var(constants::ENV_APERTURE_CONFIG_DIR).map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                aperture_cli::cli::errors::print_error_with_json(&e, json_errors);
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    );

    if let Err(e) = run_command(cli, &manager, &output).await {
        aperture_cli::cli::errors::print_error_with_json(&e, json_errors);
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
async fn run_command(
    cli: Cli,
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    use aperture_cli::cli::commands::{api, config, docs, search};

    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Add {
                name,
                file_or_url,
                force,
                strict,
            } => {
                let name = validate_api_name(&name)?;
                manager
                    .add_spec_auto(&name, &file_or_url, force, strict)
                    .await?;
                output.success(format!("Spec '{name}' added successfully."));
            }
            ConfigCommands::List { verbose } => {
                let specs = manager.list_specs()?;
                if specs.is_empty() {
                    output.info("No API specifications found.");
                } else {
                    output.info("Registered API specifications:");
                    config::list_specs_with_details(manager, specs, verbose, output);
                }
            }
            ConfigCommands::Remove { name } => {
                let name = validate_api_name(&name)?;
                manager.remove_spec(&name)?;
                output.success(format!("Spec '{name}' removed successfully."));
            }
            ConfigCommands::Edit { name } => {
                let name = validate_api_name(&name)?;
                manager.edit_spec(&name)?;
                output.success(format!("Opened spec '{name}' in editor."));
            }
            ConfigCommands::SetUrl { name, url, env } => {
                let name = validate_api_name(&name)?;
                manager.set_url(&name, &url, env.as_deref())?;
                if let Some(environment) = env {
                    output.success(format!(
                        "Set base URL for '{name}' in environment '{environment}': {url}"
                    ));
                } else {
                    output.success(format!("Set base URL for '{name}': {url}"));
                }
            }
            ConfigCommands::GetUrl { name } => {
                let name = validate_api_name(&name)?;
                let (base_override, env_urls, resolved) = manager.get_url(&name)?;
                config::print_url_configuration(
                    &name,
                    base_override.as_deref(),
                    &env_urls,
                    &resolved,
                    output,
                );
            }
            ConfigCommands::ListUrls {} => {
                let all_urls = manager.list_urls()?;
                if all_urls.is_empty() {
                    output.info("No base URLs configured.");
                    return Ok(());
                }
                output.info("Configured base URLs:");
                for (api_name, (base_override, env_urls)) in all_urls {
                    config::print_api_url_entry(
                        &api_name,
                        base_override.as_deref(),
                        &env_urls,
                        output,
                    );
                }
            }
            ConfigCommands::Reinit { context, all } => {
                if all {
                    config::reinit_all_specs(manager, output)?;
                    return Ok(());
                }
                let Some(spec_name) = context else {
                    eprintln!("Error: Either specify a spec name or use --all flag");
                    std::process::exit(1);
                };
                let spec_name = validate_api_name(&spec_name)?;
                config::reinit_spec(manager, &spec_name, output)?;
            }
            ConfigCommands::ClearCache { api_name, all } => {
                if let Some(ref name) = api_name {
                    validate_api_name(name)?;
                }
                config::clear_response_cache(manager, api_name.as_deref(), all, output).await?;
            }
            ConfigCommands::CacheStats { api_name } => {
                if let Some(ref name) = api_name {
                    validate_api_name(name)?;
                }
                config::show_cache_stats(manager, api_name.as_deref(), output).await?;
            }
            ConfigCommands::SetSecret {
                api_name,
                scheme_name,
                env,
                interactive,
            } => {
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
            }
            ConfigCommands::ListSecrets { api_name } => {
                let api_name = validate_api_name(&api_name)?;
                let secrets = manager.list_secrets(&api_name)?;
                if secrets.is_empty() {
                    output.info(format!("No secrets configured for API '{api_name}'"));
                } else {
                    config::print_secrets_list(&api_name, secrets, output);
                }
            }
            ConfigCommands::RemoveSecret {
                api_name,
                scheme_name,
            } => {
                let api_name = validate_api_name(&api_name)?;
                manager.remove_secret(&api_name, &scheme_name)?;
                output.success(format!(
                    "Removed secret configuration for scheme '{scheme_name}' from API '{api_name}'"
                ));
            }
            ConfigCommands::ClearSecrets { api_name, force } => {
                let api_name = validate_api_name(&api_name)?;
                let secrets = manager.list_secrets(&api_name)?;
                if secrets.is_empty() {
                    output.info(format!("No secrets configured for API '{api_name}'"));
                    return Ok(());
                }
                if force {
                    manager.clear_secrets(&api_name)?;
                    output.success(format!(
                        "Cleared all secret configurations for API '{api_name}'"
                    ));
                    return Ok(());
                }
                output.info(format!(
                    "This will remove all {} secret configuration(s) for API '{api_name}':",
                    secrets.len()
                ));
                for scheme_name in secrets.keys() {
                    output.info(format!("  - {scheme_name}"));
                }
                if !confirm("Are you sure you want to continue?")? {
                    output.info("Operation cancelled");
                    return Ok(());
                }
                manager.clear_secrets(&api_name)?;
                output.success(format!(
                    "Cleared all secret configurations for API '{api_name}'"
                ));
            }
            ConfigCommands::Set { key, value } => {
                use aperture_cli::config::settings::{SettingKey, SettingValue};
                let setting_key: SettingKey = key.parse()?;
                let setting_value = SettingValue::parse_for_key(setting_key, &value)?;
                manager.set_setting(&setting_key, &setting_value)?;
                output.success(format!("Set {key} = {value}"));
            }
            ConfigCommands::Get { key, json } => {
                use aperture_cli::config::settings::SettingKey;
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
            }
            ConfigCommands::Settings { json } => {
                let settings = manager.list_settings()?;
                config::print_settings_list(settings, json, output)?;
            }
        },
        Commands::ListCommands { ref context } => {
            let context = validate_api_name(context)?;
            docs::list_commands(&context, output)?;
        }
        Commands::Api {
            ref context,
            ref args,
        } => {
            let context = validate_api_name(context)?;
            api::execute_api_command(&context, args.clone(), &cli).await?;
        }
        Commands::Search {
            ref query,
            ref api,
            verbose,
        } => {
            let validated_api = api.as_deref().map(validate_api_name).transpose()?;
            search::execute_search_command(
                manager,
                query,
                validated_api.as_deref(),
                verbose,
                output,
            )?;
        }
        Commands::Exec { ref args } => {
            api::execute_shortcut_command(manager, args.clone(), &cli).await?;
        }
        Commands::Docs {
            ref api,
            ref tag,
            ref operation,
            enhanced,
        } => {
            let validated_api = api.as_deref().map(validate_api_name).transpose()?;
            docs::execute_help_command(
                manager,
                validated_api.as_deref(),
                tag.as_deref(),
                operation.as_deref(),
                enhanced,
                output,
            )?;
        }
        Commands::Overview { ref api, all } => {
            let validated_api = api.as_deref().map(validate_api_name).transpose()?;
            docs::execute_overview_command(manager, validated_api.as_deref(), all, output)?;
        }
    }
    Ok(())
}
