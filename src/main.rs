use aperture_cli::agent;
use aperture_cli::cli::{Cli, Commands, ConfigCommands};
use aperture_cli::config::manager::{get_config_dir, ConfigManager};
use aperture_cli::engine::{executor, generator, loader};
use aperture_cli::error::Error;
use aperture_cli::fs::OsFileSystem;
use clap::Parser;
use std::fs;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let json_errors = cli.json_errors;

    let manager = std::env::var("APERTURE_CONFIG_DIR").map_or_else(
        |_| match ConfigManager::new() {
            Ok(manager) => manager,
            Err(e) => {
                print_error_with_json(&e, json_errors);
                std::process::exit(1);
            }
        },
        |config_dir| ConfigManager::with_fs(OsFileSystem, PathBuf::from(config_dir)),
    );

    if let Err(e) = run_command(cli, &manager).await {
        print_error_with_json(&e, json_errors);
        std::process::exit(1);
    }
}

async fn run_command(cli: Cli, manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
    match cli.command {
        Commands::Config { command } => match command {
            ConfigCommands::Add { name, file, force } => {
                manager.add_spec(&name, &file, force)?;
                println!("Spec '{name}' added successfully.");
            }
            ConfigCommands::List {} => {
                let specs = manager.list_specs()?;
                if specs.is_empty() {
                    println!("No API specifications found.");
                } else {
                    println!("Registered API specifications:");
                    for spec in specs {
                        println!("- {spec}");
                    }
                }
            }
            ConfigCommands::Remove { name } => {
                manager.remove_spec(&name)?;
                println!("Spec '{name}' removed successfully.");
            }
            ConfigCommands::Edit { name } => {
                manager.edit_spec(&name)?;
                println!("Opened spec '{name}' in editor.");
            }
            ConfigCommands::SetUrl { name, url, env } => {
                manager.set_url(&name, &url, env.as_deref())?;
                if let Some(environment) = env {
                    println!("Set base URL for '{name}' in environment '{environment}': {url}");
                } else {
                    println!("Set base URL for '{name}': {url}");
                }
            }
            ConfigCommands::GetUrl { name } => {
                let (base_override, env_urls, resolved) = manager.get_url(&name)?;

                println!("Base URL configuration for '{name}':");
                if let Some(base) = base_override {
                    println!("  Base override: {base}");
                } else {
                    println!("  Base override: (none)");
                }

                if !env_urls.is_empty() {
                    println!("  Environment URLs:");
                    for (env, url) in env_urls {
                        println!("    {env}: {url}");
                    }
                }

                println!("\nResolved URL (current): {resolved}");

                if let Ok(current_env) = std::env::var("APERTURE_ENV") {
                    println!("(Using APERTURE_ENV={current_env})");
                }
            }
            ConfigCommands::ListUrls {} => {
                let all_urls = manager.list_urls()?;

                if all_urls.is_empty() {
                    println!("No base URLs configured.");
                } else {
                    println!("Configured base URLs:");
                    for (api_name, (base_override, env_urls)) in all_urls {
                        println!("\n{api_name}:");
                        if let Some(base) = base_override {
                            println!("  Base override: {base}");
                        }
                        if !env_urls.is_empty() {
                            println!("  Environment URLs:");
                            for (env, url) in env_urls {
                                println!("    {env}: {url}");
                            }
                        }
                    }
                }
            }
        },
        Commands::Api {
            ref context,
            ref args,
        } => {
            execute_api_command(context, args.clone(), &cli).await?;
        }
    }

    Ok(())
}

async fn execute_api_command(context: &str, args: Vec<String>, cli: &Cli) -> Result<(), Error> {
    // Get the cache directory - respecting APERTURE_CONFIG_DIR if set
    let config_dir = if let Ok(dir) = std::env::var("APERTURE_CONFIG_DIR") {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(".cache");

    // Create config manager and load global config
    let manager = ConfigManager::with_fs(OsFileSystem, config_dir.clone());
    let global_config = manager.load_global_config().ok();

    // Load the cached spec for the context
    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::SpecNotFound {
            name: context.to_string(),
        },
        _ => e,
    })?;

    // Handle --describe-json flag - output capability manifest and exit
    if cli.describe_json {
        // Load the original spec file for complete metadata
        let specs_dir = config_dir.join("specs");
        let spec_path = specs_dir.join(format!("{context}.yaml"));

        if !spec_path.exists() {
            return Err(Error::SpecNotFound {
                name: context.to_string(),
            });
        }

        let spec_content = fs::read_to_string(&spec_path)?;
        let openapi_spec: openapiv3::OpenAPI = serde_yaml::from_str(&spec_content)
            .map_err(|e| Error::Config(format!("Failed to parse OpenAPI spec: {e}")))?;

        // Generate manifest from the original spec with all metadata
        let manifest = agent::generate_capability_manifest_from_openapi(
            context,
            &openapi_spec,
            global_config.as_ref(),
        )?;
        println!("{manifest}");
        return Ok(());
    }

    // Generate the dynamic command tree
    let command = generator::generate_command_tree(&spec);

    // Parse the arguments against the dynamic command
    let matches = command
        .try_get_matches_from(std::iter::once("api".to_string()).chain(args))
        .map_err(|e| Error::InvalidCommand {
            context: context.to_string(),
            reason: e.to_string(),
        })?;

    // Execute the request with agent flags
    executor::execute_request(
        &spec,
        &matches,
        None, // base_url (None = use BaseUrlResolver)
        cli.dry_run,
        cli.idempotency_key.as_deref(),
        global_config.as_ref(),
    )
    .await
    .map_err(|e| match &e {
        Error::Network(req_err) => {
            if req_err.is_connect() {
                e.with_context("Failed to connect to API server")
            } else if req_err.is_timeout() {
                e.with_context("Request timed out")
            } else {
                e
            }
        }
        _ => e,
    })?;

    Ok(())
}

/// Prints an error message, either as JSON or user-friendly format
fn print_error_with_json(error: &Error, json_format: bool) {
    if json_format {
        let json_error = error.to_json();
        if let Ok(json_output) = serde_json::to_string_pretty(&json_error) {
            eprintln!("{json_output}");
        } else {
            // Fallback to regular error if JSON serialization fails
            print_error(error);
        }
    } else {
        print_error(error);
    }
}

/// Prints a user-friendly error message with context and suggestions
#[allow(clippy::too_many_lines)]
fn print_error(error: &Error) {
    match error {
        Error::Config(msg) => {
            eprintln!("Configuration Error\n{msg}");
        }
        Error::Io(io_err) => match io_err.kind() {
            std::io::ErrorKind::NotFound => {
                eprintln!("File Not Found\n{io_err}\n\nHint: Check that the file path is correct and the file exists.");
            }
            std::io::ErrorKind::PermissionDenied => {
                eprintln!("Permission Denied\n{io_err}\n\nHint: Check file permissions or run with appropriate privileges.");
            }
            _ => {
                eprintln!("File System Error\n{io_err}");
            }
        },
        Error::Network(req_err) => {
            if req_err.is_connect() {
                eprintln!("Connection Error\n{req_err}\n\nHint: Check that the API server is running and accessible.");
            } else if req_err.is_timeout() {
                eprintln!("Request Timeout\n{req_err}\n\nHint: The API server may be slow or unresponsive. Try again later.");
            } else if req_err.is_status() {
                if let Some(status) = req_err.status() {
                    match status.as_u16() {
                        401 => eprintln!("Authentication Error (401)\n{req_err}\n\nHint: Check your API credentials and authentication configuration."),
                        403 => eprintln!("Authorization Error (403)\n{req_err}\n\nHint: Your credentials may be valid but lack permission for this operation."),
                        404 => eprintln!("Resource Not Found (404)\n{req_err}\n\nHint: Check that the API endpoint and parameters are correct."),
                        429 => eprintln!("Rate Limited (429)\n{req_err}\n\nHint: You're making requests too quickly. Wait before trying again."),
                        500..=599 => eprintln!("Server Error ({})\n{req_err}\n\nHint: The API server is experiencing issues. Try again later.", status.as_u16()),
                        _ => eprintln!("HTTP Error ({})\n{req_err}", status.as_u16()),
                    }
                } else {
                    eprintln!("HTTP Error\n{req_err}");
                }
            } else {
                eprintln!("Network Error\n{req_err}");
            }
        }
        Error::Yaml(yaml_err) => {
            eprintln!("YAML Parsing Error\n{yaml_err}\n\nHint: Check that your OpenAPI specification is valid YAML syntax.");
        }
        Error::Json(json_err) => {
            eprintln!("JSON Parsing Error\n{json_err}\n\nHint: Check that your request body or response contains valid JSON.");
        }
        Error::Validation(msg) => {
            eprintln!("Validation Error\n{msg}\n\nHint: Check that your OpenAPI specification follows the required format.");
        }
        Error::Toml(toml_err) => {
            eprintln!("TOML Parsing Error\n{toml_err}\n\nHint: Check that your configuration file is valid TOML syntax.");
        }
        Error::SpecNotFound { name } => {
            eprintln!("API Specification Not Found\n{error}\n\nHint: Use 'aperture config list' to see available specifications\n      or 'aperture config add {name} <file>' to add this specification.");
        }
        Error::SpecAlreadyExists { .. } => {
            eprintln!("Specification Already Exists\n{error}");
        }
        Error::CachedSpecNotFound { .. } => {
            eprintln!("Cached Specification Not Found\n{error}");
        }
        Error::CachedSpecCorrupted { .. } => {
            eprintln!("Cached Specification Corrupted\n{error}\n\nHint: Try removing and re-adding the specification.");
        }
        Error::SecretNotSet { env_var, .. } => {
            eprintln!("Authentication Secret Not Set\n{error}\n\nHint: Set the environment variable: export {env_var}=<your-secret>");
        }
        Error::InvalidHeaderFormat { .. }
        | Error::InvalidHeaderName { .. }
        | Error::InvalidHeaderValue { .. }
        | Error::EmptyHeaderName => {
            eprintln!("Invalid Header\n{error}");
        }
        Error::EditorNotSet => {
            eprintln!(
                "Editor Not Set\n{error}\n\nHint: Set your preferred editor: export EDITOR=vim"
            );
        }
        Error::EditorFailed { .. } => {
            eprintln!("Editor Failed\n{error}");
        }
        Error::InvalidHttpMethod { .. } => {
            eprintln!("Invalid HTTP Method\n{error}");
        }
        Error::MissingPathParameter { .. } => {
            eprintln!("Missing Path Parameter\n{error}");
        }
        Error::UnsupportedAuthScheme { .. } | Error::UnsupportedSecurityScheme { .. } => {
            eprintln!("Unsupported Security Scheme\n{error}");
        }
        Error::SerializationError { .. } => {
            eprintln!("Serialization Error\n{error}");
        }
        Error::InvalidConfig { .. } => {
            eprintln!("Invalid Configuration\n{error}\n\nHint: Check the TOML syntax in your configuration file.");
        }
        Error::HomeDirectoryNotFound => {
            eprintln!("Home Directory Not Found\n{error}\n\nHint: Ensure HOME environment variable is set.");
        }
        Error::InvalidJsonBody { .. } => {
            eprintln!("Invalid JSON Body\n{error}\n\nHint: Check your JSON syntax and ensure all quotes are properly escaped.");
        }
        Error::RequestFailed { .. } | Error::ResponseReadError { .. } => {
            eprintln!("Request Failed\n{error}");
        }
        Error::HttpError { status, .. } => {
            eprintln!("HTTP Error ({status})\n{error}");
        }
        Error::InvalidCommand { context, .. } => {
            eprintln!("Invalid Command\n{error}\n\nHint: Use 'aperture api {context} --help' to see available commands.");
        }
        Error::OperationNotFound => {
            eprintln!("Operation Not Found\n{error}\n\nHint: Check that the command matches an available operation.");
        }
        Error::InvalidIdempotencyKey => {
            eprintln!("Invalid Idempotency Key\n{error}\n\nHint: Idempotency key must be a valid header value.");
        }
        Error::Anyhow(err) => {
            eprintln!("💥 Unexpected Error\n{err}\n\nHint: This may be a bug. Please report it with the command you were running.");
        }
    }
}
