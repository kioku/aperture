//! Handlers for `aperture docs`, `aperture overview`, and `aperture list-commands`.

use crate::cache::models::CachedSpec;
use crate::config::manager::{get_config_dir, ConfigManager};
use crate::constants;
use crate::docs::{DocumentationGenerator, HelpFormatter};
use crate::engine::loader;
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::output::Output;
use std::path::PathBuf;

pub fn list_commands(context: &str, output: &Output) -> Result<(), Error> {
    let config_dir = if let Ok(dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        PathBuf::from(dir)
    } else {
        get_config_dir()?
    };
    let cache_dir = config_dir.join(constants::DIR_CACHE);
    let spec = loader::load_cached_spec(&cache_dir, context).map_err(|e| match e {
        Error::Io(_) => Error::spec_not_found(context),
        _ => e,
    })?;
    let formatted_output = HelpFormatter::format_command_list(&spec);
    // ast-grep-ignore: no-println
    println!("{formatted_output}");
    output.tip(format!(
        "Use 'aperture docs {context}' for detailed API documentation"
    ));
    output.tip(format!(
        "Use 'aperture search <term> --api {context}' to find specific operations"
    ));
    output.tip("Use shortcuts: 'aperture exec <operation-id> --help'");
    Ok(())
}

/// Execute help command with enhanced documentation
pub fn execute_help_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
    enhanced: bool,
    output: &Output,
) -> Result<(), Error> {
    match (api_name, tag, operation) {
        (None, None, None) => {
            let specs = load_all_specs(manager)?;
            let doc_gen = DocumentationGenerator::new(specs);
            // ast-grep-ignore: no-println
            println!("{}", doc_gen.generate_interactive_menu());
        }
        (Some(api), tag_opt, operation_opt) => {
            let specs = load_all_specs(manager)?;
            let doc_gen = DocumentationGenerator::new(specs);
            match (tag_opt, operation_opt) {
                (None, None) => {
                    let overview = doc_gen.generate_api_overview(api)?;
                    // ast-grep-ignore: no-println
                    println!("{overview}");
                }
                (Some(tag), Some(op)) => {
                    let help = doc_gen.generate_command_help(api, tag, op)?;
                    if enhanced {
                        // ast-grep-ignore: no-println
                        println!("{help}");
                    } else {
                        // ast-grep-ignore: no-println
                        println!("{}", help.lines().take(20).collect::<Vec<_>>().join("\n"));
                        output.tip("Use --enhanced for full documentation with examples");
                    }
                }
                _ => {
                    // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
                    // ast-grep-ignore: no-println
                    eprintln!("Invalid docs command. Usage:");
                    // ast-grep-ignore: no-println
                    eprintln!("  aperture docs                        # Interactive menu");
                    // ast-grep-ignore: no-println
                    eprintln!("  aperture docs <api>                  # API overview");
                    // ast-grep-ignore: no-println
                    eprintln!("  aperture docs <api> <tag> <operation> # Command help");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
            // ast-grep-ignore: no-println
            eprintln!("Invalid help command arguments");
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Execute overview command
#[allow(clippy::too_many_lines)]
pub fn execute_overview_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    all: bool,
    output: &Output,
) -> Result<(), Error> {
    if !all {
        let Some(api) = api_name else {
            // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
            // ast-grep-ignore: no-println
            eprintln!("Error: Must specify API name or use --all flag");
            // ast-grep-ignore: no-println
            eprintln!("Usage:");
            // ast-grep-ignore: no-println
            eprintln!("  aperture overview <api>");
            // ast-grep-ignore: no-println
            eprintln!("  aperture overview --all");
            std::process::exit(1);
        };
        let specs = load_all_specs(manager)?;
        let doc_gen = DocumentationGenerator::new(specs);
        let overview = doc_gen.generate_api_overview(api)?;
        // ast-grep-ignore: no-println
        println!("{overview}");
        return Ok(());
    }

    let specs = load_all_specs(manager)?;
    if specs.is_empty() {
        output.info("No API specifications configured.");
        output.info("Use 'aperture config add <name> <spec-file>' to get started.");
        return Ok(());
    }

    // ast-grep-ignore: no-println
    println!("All APIs Overview\n");
    // ast-grep-ignore: no-println
    println!("{}", "=".repeat(60));
    for (api_name, spec) in &specs {
        // ast-grep-ignore: no-println
        println!("\n** {} ** (v{})", spec.name, spec.version);
        if let Some(ref base_url) = spec.base_url {
            // ast-grep-ignore: no-println
            println!("   Base URL: {base_url}");
        }
        let operation_count = spec.commands.len();
        // ast-grep-ignore: no-println
        println!("   Operations: {operation_count}");
        let mut method_counts = std::collections::BTreeMap::new();
        for command in &spec.commands {
            *method_counts.entry(command.method.clone()).or_insert(0) += 1;
        }
        let method_summary: Vec<String> = method_counts
            .iter()
            .map(|(method, count)| format!("{method}: {count}"))
            .collect();
        // ast-grep-ignore: no-println
        println!("   Methods: {}", method_summary.join(", "));
        // ast-grep-ignore: no-println
        println!("   Quick start: aperture list-commands {api_name}");
    }
    // ast-grep-ignore: no-println
    println!("\n{}", "=".repeat(60));
    output.tip("Use 'aperture overview <api>' for detailed information about a specific API");
    Ok(())
}

/// Load all cached specs from the manager
pub fn load_all_specs(
    manager: &ConfigManager<OsFileSystem>,
) -> Result<std::collections::BTreeMap<String, CachedSpec>, Error> {
    let specs = manager.list_specs()?;
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let mut all_specs = std::collections::BTreeMap::new();
    for spec_name in &specs {
        match loader::load_cached_spec(&cache_dir, spec_name) {
            Ok(spec) => {
                all_specs.insert(spec_name.clone(), spec);
            }
            Err(e) => tracing::warn!(spec = spec_name, error = %e, "could not load spec"),
        }
    }
    Ok(all_specs)
}
