//! Handler for `aperture search`.

use crate::config::manager::ConfigManager;
use crate::constants;
use crate::discovery_style::DiscoveryStyle;
use crate::engine::loader;
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::output::Output;
use crate::search::CommandSearcher;

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
        // ast-grep-ignore: no-println
        println!("{line}");
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
