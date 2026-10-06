//! Handler for `aperture search`.

use crate::cli::DiscoveryFormat;
use crate::config::manager::ConfigManager;
use crate::constants;
use crate::discovery_style::DiscoveryStyle;
use crate::engine::loader;
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::output::{write_stdout_line, Output};
use crate::search::{CommandSearchResult, CommandSearcher};

/// Stable, spec-derived discovery data; excludes cache and execution internals.
#[derive(serde::Serialize)]
struct SearchOutput<'a> {
    query: &'a str,
    api_filter: Option<&'a str>,
    results: Vec<SearchResult<'a>>,
}

#[derive(serde::Serialize)]
struct SearchResult<'a> {
    api_context: &'a str,
    operation_id: &'a str,
    command_path: &'a str,
    method: &'a str,
    path: &'a str,
    summary: Option<&'a str>,
    score: i64,
    highlights: &'a [String],
}

impl<'a> From<&'a CommandSearchResult> for SearchResult<'a> {
    fn from(result: &'a CommandSearchResult) -> Self {
        Self {
            api_context: &result.api_context,
            operation_id: &result.command.operation_id,
            command_path: &result.command_path,
            method: &result.command.method,
            path: &result.command.path,
            summary: result.command.summary.as_deref(),
            score: result.score,
            highlights: &result.highlights,
        }
    }
}

/// Search using the selected discovery format. Text retains the existing handler.
///
/// # Errors
/// Returns configuration, search validation, serialization, or output errors.
pub fn execute_search_command_with_format(
    manager: &ConfigManager<OsFileSystem>,
    query: &str,
    api_filter: Option<&str>,
    verbose: bool,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
    if matches!(format, DiscoveryFormat::Text) {
        return execute_search_command(manager, query, api_filter, verbose, output);
    }
    let specs = manager.list_specs()?;
    let all_specs = load_search_specs(manager, api_filter, &specs);
    let results = CommandSearcher::new().search(&all_specs, query, api_filter)?;
    let data = SearchOutput {
        query,
        api_filter,
        results: results.iter().map(SearchResult::from).collect(),
    };
    write_stdout_line(&serde_json::to_string_pretty(&data)?)
}

pub fn execute_search_command(
    manager: &ConfigManager<OsFileSystem>,
    query: &str,
    api_filter: Option<&str>,
    verbose: bool,
    output: &Output,
) -> Result<(), Error> {
    let specs = manager.list_specs()?;
    if specs.is_empty() {
        output.info("No API specifications found. Use 'aperture config api add' to register APIs.");
        return Ok(());
    }

    let all_specs = load_search_specs(manager, api_filter, &specs);
    if all_specs.is_empty() {
        match api_filter {
            Some(filter) => {
                output.info(format!("API '{filter}' not found or could not be loaded."));
            }
            None => output.info("No API specifications could be loaded."),
        }
        return Ok(());
    }

    let searcher = CommandSearcher::new();
    let results = searcher.search(&all_specs, query, api_filter)?;
    let style = DiscoveryStyle::for_stdout();
    let formatted_results =
        crate::search::format_search_results_with_style(&results, verbose, style);
    for line in formatted_results {
        write_stdout_line(&line)?;
    }
    Ok(())
}

fn load_search_specs(
    manager: &ConfigManager<OsFileSystem>,
    api_filter: Option<&str>,
    specs: &[String],
) -> std::collections::BTreeMap<String, crate::cache::models::CachedSpec> {
    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let mut all_specs = std::collections::BTreeMap::new();

    for spec_name in specs {
        if api_filter.is_some_and(|filter| spec_name != filter) {
            continue;
        }

        match loader::load_cached_spec(&cache_dir, spec_name) {
            Ok(spec) => {
                all_specs.insert(spec_name.clone(), spec);
            }
            Err(e) => tracing::warn!(spec = spec_name, error = %e, "could not load spec"),
        }
    }

    all_specs
}
