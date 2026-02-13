//! Legacy clap-based execution adapter.
//!
//! This module preserves the `execute_request(...)` API used by older callers
//! and tests, while delegating to the new domain-based execution pipeline:
//! `ArgMatches -> OperationCall/ExecutionContext -> execute() -> render`.

use crate::cache::models::CachedSpec;
use crate::cli::OutputFormat;
use crate::config::models::GlobalConfig;
use crate::engine::executor::RetryContext;
use crate::error::Error;
use crate::response_cache::CacheConfig;
use clap::ArgMatches;

/// Legacy wrapper that translates `ArgMatches` into domain types and delegates
/// to [`crate::engine::executor::execute`]. Retained for backward compatibility.
///
/// # Errors
///
/// Returns errors for authentication failures, network issues, or JQ filter errors.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::missing_panics_doc)]
pub async fn execute_request(
    spec: &CachedSpec,
    matches: &ArgMatches,
    base_url: Option<&str>,
    dry_run: bool,
    idempotency_key: Option<&str>,
    global_config: Option<&GlobalConfig>,
    output_format: &OutputFormat,
    jq_filter: Option<&str>,
    cache_config: Option<&CacheConfig>,
    capture_output: bool,
    retry_context: Option<&RetryContext>,
) -> Result<Option<String>, Error> {
    use crate::cli::translate;
    use crate::invocation::ExecutionContext;

    // Check --show-examples flag (CLI-only concern).
    // NOTE: The primary path through `execute_api_command` also checks this
    // flag before reaching here.  This duplicate check is intentional so that
    // callers of the legacy `execute_request` API (tests, batch) still get
    // correct behavior without depending on an outer guard.
    if translate::has_show_examples_flag(matches) {
        let operation_id = translate::matches_to_operation_id(spec, matches)?;
        let operation = spec
            .commands
            .iter()
            .find(|cmd| cmd.operation_id == operation_id)
            .ok_or_else(|| Error::spec_not_found(&spec.name))?;
        crate::cli::render::render_examples(operation);
        return Ok(None);
    }

    // Translate ArgMatches → OperationCall
    let call = translate::matches_to_operation_call(spec, matches)?;

    // Build ExecutionContext from the individual parameters
    let ctx = ExecutionContext {
        dry_run,
        idempotency_key: idempotency_key.map(String::from),
        cache_config: cache_config.cloned(),
        retry_context: retry_context.cloned(),
        base_url: base_url.map(String::from),
        global_config: global_config.cloned(),
        server_var_args: translate::extract_server_var_args(matches),
    };

    // Execute using the new domain-type API
    let result = crate::engine::executor::execute(spec, call, ctx).await?;

    // Render to string or stdout based on capture_output
    if capture_output {
        crate::cli::render::render_result_to_string(&result, output_format, jq_filter)
    } else {
        crate::cli::render::render_result(&result, output_format, jq_filter)?;
        Ok(None)
    }
}
