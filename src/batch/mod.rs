pub mod capture;
pub mod graph;
pub mod interpolation;

use crate::cache::models::CachedSpec;
use crate::config::models::GlobalConfig;
use crate::duration::parse_duration;
use crate::engine::executor::RetryContext;
use crate::engine::generator;
use crate::error::Error;
use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Configuration for batch processing operations
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of concurrent requests
    pub max_concurrency: usize,
    /// Rate limit: requests per second
    pub rate_limit: Option<u32>,
    /// Whether to continue processing if a request fails
    pub continue_on_error: bool,
    /// Whether to show progress during processing
    pub show_progress: bool,
    /// Whether to suppress individual operation outputs
    pub suppress_output: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 5,
            rate_limit: None,
            continue_on_error: true,
            show_progress: true,
            suppress_output: false,
        }
    }
}

/// A single batch operation definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BatchOperation {
    /// Unique identifier for this operation (optional for independent ops, required when
    /// using `capture`, `capture_append`, or `depends_on`)
    pub id: Option<String>,
    /// The command arguments to execute (e.g., `["users", "get", "--user-id", "123"]`)
    pub args: Vec<String>,
    /// Optional description for this operation
    pub description: Option<String>,
    /// Custom headers for this specific operation
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    /// Whether to use cache for this operation (overrides global cache setting)
    pub use_cache: Option<bool>,
    /// Maximum number of retry attempts for this operation (overrides global retry setting)
    #[serde(default)]
    pub retry: Option<u32>,
    /// Initial delay between retries (e.g., "500ms", "1s")
    #[serde(default)]
    pub retry_delay: Option<String>,
    /// Maximum delay cap between retries (e.g., "30s", "1m")
    #[serde(default)]
    pub retry_max_delay: Option<String>,
    /// Force retry on non-idempotent requests without an idempotency key
    #[serde(default)]
    pub force_retry: bool,

    /// Capture scalar values from the response using JQ syntax.
    /// Maps variable name → JQ query (e.g., `{"user_id": ".id"}`).
    /// Captured values are available for `{{variable}}` interpolation in subsequent operations.
    #[serde(default)]
    pub capture: Option<std::collections::HashMap<String, String>>,

    /// Append extracted values to a named list using JQ syntax.
    /// Maps list name → JQ query. The list interpolates as a JSON array literal.
    /// Enables fan-out/aggregate patterns where N operations feed into a terminal call.
    #[serde(default)]
    pub capture_append: Option<std::collections::HashMap<String, String>>,

    /// Explicit dependency on other operations by their `id`.
    /// This operation will not execute until all dependencies have completed.
    /// Dependencies can also be inferred from `{{variable}}` usage in `args`.
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,

    /// Read the request body from this file path instead of embedding it in
    /// `args`. Equivalent to passing `--body-file <path>` in `args`, but
    /// avoids quoting issues with long or prose-heavy JSON payloads.
    /// Mutually exclusive with a `--body` or `--body-file` entry in `args`.
    #[serde(default)]
    pub body_file: Option<String>,
}

/// Batch file format containing multiple operations
#[derive(Debug, Serialize, Deserialize)]
pub struct BatchFile {
    /// Metadata about this batch
    pub metadata: Option<BatchMetadata>,
    /// List of operations to execute
    pub operations: Vec<BatchOperation>,
}

/// Metadata for a batch file
#[derive(Debug, Serialize, Deserialize)]
pub struct BatchMetadata {
    /// Name/description of this batch
    pub name: Option<String>,
    /// Version of the batch file format
    pub version: Option<String>,
    /// Description of what this batch does
    pub description: Option<String>,
    /// Default configuration for all operations in this batch
    pub defaults: Option<BatchDefaults>,
}

/// Default configuration for batch operations
#[derive(Debug, Serialize, Deserialize)]
pub struct BatchDefaults {
    /// Default headers to apply to all operations
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
    /// Default cache setting for all operations
    pub use_cache: Option<bool>,
}

/// Result of a single batch operation
#[derive(Debug)]
pub struct BatchOperationResult {
    /// The operation that was executed
    pub operation: BatchOperation,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if the operation failed
    pub error: Option<String>,
    /// Response body if the operation succeeded
    pub response: Option<String>,
    /// Time taken to execute this operation
    pub duration: std::time::Duration,
}

/// Result of an entire batch execution
#[derive(Debug)]
pub struct BatchResult {
    /// Results for each operation
    pub results: Vec<BatchOperationResult>,
    /// Total time taken for the entire batch
    pub total_duration: std::time::Duration,
    /// Number of successful operations
    pub success_count: usize,
    /// Number of failed operations
    pub failure_count: usize,
}

/// Batch processor for executing multiple API operations
pub struct BatchProcessor {
    config: BatchConfig,
    rate_limiter: Option<Arc<DefaultDirectRateLimiter>>,
    semaphore: Arc<Semaphore>,
}

impl BatchProcessor {
    /// Creates a new batch processor with the given configuration
    ///
    /// # Panics
    ///
    /// Panics if the rate limit is configured as 0 (which would be invalid)
    #[must_use]
    pub fn new(config: BatchConfig) -> Self {
        let rate_limiter = config.rate_limit.map(|limit| {
            Arc::new(RateLimiter::direct(Quota::per_second(
                NonZeroU32::new(limit).unwrap_or(NonZeroU32::new(1).expect("1 is non-zero")),
            )))
        });

        let semaphore = Arc::new(Semaphore::new(config.max_concurrency));

        Self {
            config,
            rate_limiter,
            semaphore,
        }
    }

    /// Parses a batch file from the given path
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The file is not valid JSON or YAML
    /// - The file structure doesn't match the expected `BatchFile` format
    pub async fn parse_batch_file(path: &Path) -> Result<BatchFile, Error> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| Error::io_error(format!("Failed to read batch file: {e}")))?;

        // Try to parse as JSON first, then YAML
        if let Ok(batch_file) = serde_json::from_str::<BatchFile>(&content) {
            return Ok(batch_file);
        }

        if let Ok(batch_file) = serde_yaml::from_str::<BatchFile>(&content) {
            return Ok(batch_file);
        }

        Err(Error::validation_error(format!(
            "Failed to parse batch file as JSON or YAML: {}",
            path.display()
        )))
    }

    /// Executes a batch of operations.
    ///
    /// If the batch uses dependency features (`capture`, `capture_append`, or
    /// `depends_on`), operations are executed sequentially in topological order
    /// with variable interpolation and atomic failure semantics.
    ///
    /// Otherwise, operations are executed concurrently using the original
    /// parallel execution strategy.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The dependency graph is invalid (cycles, missing refs)
    /// - Any operation fails in dependent mode (atomic execution)
    /// - Any operation fails and `continue_on_error` is false in concurrent mode
    ///
    /// # Panics
    ///
    /// Panics if the semaphore is poisoned (should not happen in normal operation)
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_batch(
        &self,
        spec: &CachedSpec,
        batch_file: BatchFile,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        output_format: &crate::cli::OutputFormat,
        jq_filter: Option<&str>,
    ) -> Result<BatchResult, Error> {
        if graph::has_dependencies(&batch_file.operations) {
            self.execute_dependent_batch(
                spec,
                batch_file,
                global_config,
                base_url,
                dry_run,
                output_format,
                jq_filter,
            )
            .await
        } else {
            self.execute_concurrent_batch(
                spec,
                batch_file,
                global_config,
                base_url,
                dry_run,
                output_format,
                jq_filter,
            )
            .await
        }
    }

    /// Executes operations sequentially in dependency order with variable capture
    /// and interpolation. Halts immediately on first failure (atomic execution).
    #[allow(clippy::too_many_arguments)]
    async fn execute_dependent_batch(
        &self,
        spec: &CachedSpec,
        batch_file: BatchFile,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        _output_format: &crate::cli::OutputFormat,
        _jq_filter: Option<&str>,
    ) -> Result<BatchResult, Error> {
        let start_time = std::time::Instant::now();
        let operations = batch_file.operations;
        let total_operations = operations.len();

        let execution_order = graph::resolve_execution_order(&operations)?;

        if self.config.show_progress {
            // ast-grep-ignore: no-println
            println!("Starting dependent batch execution: {total_operations} operations");
        }

        let mut store = interpolation::VariableStore::default();
        let mut results: Vec<Option<BatchOperationResult>> =
            (0..total_operations).map(|_| None).collect();

        for &idx in &execution_order {
            let operation = &operations[idx];

            if let Some(limiter) = &self.rate_limiter {
                limiter.until_ready().await;
            }

            let result = Self::run_dependent_operation(
                spec,
                operation,
                &mut store,
                global_config,
                base_url,
                dry_run,
                self.config.show_progress,
            )
            .await;

            let failed = !result.success;
            results[idx] = Some(result);

            if failed {
                break; // Atomic execution: halt on first failure
            }
        }

        let final_results = Self::fill_skipped_results(results, &operations);
        let total_duration = start_time.elapsed();
        let success_count = final_results.iter().filter(|r| r.success).count();
        let failure_count = final_results.len() - success_count;

        if self.config.show_progress {
            // ast-grep-ignore: no-println
            println!(
                "Dependent batch completed: {success_count}/{total_operations} operations successful in {:.2}s",
                total_duration.as_secs_f64()
            );
        }

        Ok(BatchResult {
            results: final_results,
            total_duration,
            success_count,
            failure_count,
        })
    }

    /// Executes a single operation in the dependent pipeline: interpolate args,
    /// call the API, and extract captures. Returns a `BatchOperationResult`
    /// regardless of success or failure (capture failures are recorded as
    /// operation failures, not propagated).
    #[allow(clippy::too_many_arguments)]
    async fn run_dependent_operation(
        spec: &CachedSpec,
        operation: &BatchOperation,
        store: &mut interpolation::VariableStore,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        show_progress: bool,
    ) -> BatchOperationResult {
        let op_id = operation
            .id
            .as_deref()
            .unwrap_or(crate::constants::DEFAULT_OPERATION_NAME);

        let exec_op = match Self::interpolate_batch_operation(operation, store, op_id) {
            Ok(operation) => operation,
            Err(e) => {
                return Self::failed_batch_operation_result(
                    operation.clone(),
                    e.to_string(),
                    None,
                    std::time::Duration::ZERO,
                );
            }
        };

        let operation_start = std::time::Instant::now();

        // Suppress output and skip jq_filter: capture needs JSON text that
        // preserves the raw response structure regardless of caller formatting.
        let result = Self::execute_single_operation(
            spec,
            &exec_op,
            global_config,
            base_url,
            dry_run,
            &crate::cli::OutputFormat::Json,
            None,
            true,
        )
        .await;

        let duration = operation_start.elapsed();

        // From here on, store exec_op (with interpolated args) in results
        // so callers see the actual values used, not {{templates}}.
        let response = match result {
            Ok(resp) => resp,
            Err(e) => {
                Self::log_progress(show_progress, || format!("Operation '{op_id}' failed: {e}"));
                return Self::failed_batch_operation_result(exec_op, e.to_string(), None, duration);
            }
        };

        match capture::extract_captures(operation, &response, store) {
            Ok(()) => {
                Self::log_progress(show_progress, || format!("Operation '{op_id}' completed"));
                Self::successful_batch_operation_result(exec_op, response, duration)
            }
            Err(capture_err) => {
                Self::log_progress(show_progress, || {
                    format!("Operation '{op_id}' capture failed: {capture_err}")
                });
                Self::failed_batch_operation_result(
                    exec_op,
                    capture_err.to_string(),
                    Some(response),
                    duration,
                )
            }
        }
    }

    fn interpolate_batch_operation(
        operation: &BatchOperation,
        store: &interpolation::VariableStore,
        op_id: &str,
    ) -> Result<BatchOperation, Error> {
        let mut exec_op = operation.clone();
        exec_op.args = interpolation::interpolate_args(&operation.args, store, op_id)?;
        exec_op.body_file = operation
            .body_file
            .as_deref()
            .map(|path| interpolation::interpolate_string(path, store, op_id))
            .transpose()?;
        Ok(exec_op)
    }

    #[allow(clippy::missing_const_for_fn)]
    fn failed_batch_operation_result(
        operation: BatchOperation,
        error: String,
        response: Option<String>,
        duration: std::time::Duration,
    ) -> BatchOperationResult {
        BatchOperationResult {
            operation,
            success: false,
            error: Some(error),
            response,
            duration,
        }
    }

    #[allow(clippy::missing_const_for_fn)]
    fn successful_batch_operation_result(
        operation: BatchOperation,
        response: String,
        duration: std::time::Duration,
    ) -> BatchOperationResult {
        BatchOperationResult {
            operation,
            success: true,
            error: None,
            response: Some(response),
            duration,
        }
    }

    /// Conditionally prints a progress message.
    fn log_progress(show_progress: bool, msg: impl FnOnce() -> String) {
        if show_progress {
            // ast-grep-ignore: no-println
            println!("{}", msg());
        }
    }

    /// Fills `None` slots (skipped operations) with "Skipped due to prior failure".
    fn fill_skipped_results(
        results: Vec<Option<BatchOperationResult>>,
        operations: &[BatchOperation],
    ) -> Vec<BatchOperationResult> {
        results
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                r.unwrap_or_else(|| BatchOperationResult {
                    operation: operations[i].clone(),
                    success: false,
                    error: Some("Skipped due to prior failure".into()),
                    response: None,
                    duration: std::time::Duration::ZERO,
                })
            })
            .collect()
    }

    /// Original concurrent execution strategy for independent operations.
    #[allow(clippy::too_many_arguments)]
    async fn execute_concurrent_batch(
        &self,
        spec: &CachedSpec,
        batch_file: BatchFile,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        output_format: &crate::cli::OutputFormat,
        jq_filter: Option<&str>,
    ) -> Result<BatchResult, Error> {
        let start_time = std::time::Instant::now();
        let total_operations = batch_file.operations.len();
        Self::log_batch_start(self.config.show_progress, total_operations);

        let handles = self.spawn_batch_operation_handles(
            spec,
            batch_file.operations,
            global_config,
            base_url,
            dry_run,
            output_format,
            jq_filter,
        );
        let results = Self::collect_batch_operation_results(handles).await?;

        let total_duration = start_time.elapsed();
        let success_count = results.iter().filter(|r| r.success).count();
        let failure_count = results.len() - success_count;

        Self::log_batch_completion(
            self.config.show_progress,
            success_count,
            total_operations,
            total_duration,
        );

        Ok(BatchResult {
            results,
            total_duration,
            success_count,
            failure_count,
        })
    }

    fn log_batch_start(show_progress: bool, total_operations: usize) {
        if show_progress {
            // ast-grep-ignore: no-println
            println!("Starting batch execution: {total_operations} operations");
        }
    }

    fn log_batch_completion(
        show_progress: bool,
        success_count: usize,
        total_operations: usize,
        total_duration: std::time::Duration,
    ) {
        if show_progress {
            // ast-grep-ignore: no-println
            println!(
                "Batch execution completed: {}/{} operations successful in {:.2}s",
                success_count,
                total_operations,
                total_duration.as_secs_f64()
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_batch_operation_handles(
        &self,
        spec: &CachedSpec,
        operations: Vec<BatchOperation>,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        output_format: &crate::cli::OutputFormat,
        jq_filter: Option<&str>,
    ) -> Vec<tokio::task::JoinHandle<BatchOperationResult>> {
        let mut handles = Vec::new();
        for (index, operation) in operations.into_iter().enumerate() {
            let spec = spec.clone();
            let global_config = global_config.cloned();
            let base_url = base_url.map(String::from);
            let output_format = output_format.clone();
            let jq_filter = jq_filter.map(String::from);
            let semaphore = Arc::clone(&self.semaphore);
            let rate_limiter = self.rate_limiter.clone();
            let show_progress = self.config.show_progress;
            let suppress_output = self.config.suppress_output;

            handles.push(tokio::spawn(async move {
                Self::execute_batch_operation_task(
                    spec,
                    operation,
                    global_config,
                    base_url,
                    dry_run,
                    output_format,
                    jq_filter,
                    semaphore,
                    rate_limiter,
                    show_progress,
                    suppress_output,
                    index,
                )
                .await
            }));
        }
        handles
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_batch_operation_task(
        spec: CachedSpec,
        operation: BatchOperation,
        global_config: Option<GlobalConfig>,
        base_url: Option<String>,
        dry_run: bool,
        output_format: crate::cli::OutputFormat,
        jq_filter: Option<String>,
        semaphore: Arc<Semaphore>,
        rate_limiter: Option<Arc<DefaultDirectRateLimiter>>,
        show_progress: bool,
        suppress_output: bool,
        index: usize,
    ) -> BatchOperationResult {
        let _permit = semaphore
            .acquire()
            .await
            .expect("semaphore should not be closed");

        if let Some(limiter) = rate_limiter {
            limiter.until_ready().await;
        }

        let operation_start = std::time::Instant::now();
        let result = Self::execute_single_operation(
            &spec,
            &operation,
            global_config.as_ref(),
            base_url.as_deref(),
            dry_run,
            &output_format,
            jq_filter.as_deref(),
            suppress_output,
        )
        .await;
        let duration = operation_start.elapsed();

        let (success, error, response) = match result {
            Ok(resp) => {
                if show_progress {
                    // ast-grep-ignore: no-println
                    println!("Operation {} completed", index + 1);
                }
                (true, None, Some(resp))
            }
            Err(e) => {
                if show_progress {
                    // ast-grep-ignore: no-println
                    println!("Operation {} failed: {}", index + 1, e);
                }
                (false, Some(e.to_string()), None)
            }
        };

        BatchOperationResult {
            operation,
            success,
            error,
            response,
            duration,
        }
    }

    async fn collect_batch_operation_results(
        handles: Vec<tokio::task::JoinHandle<BatchOperationResult>>,
    ) -> Result<Vec<BatchOperationResult>, Error> {
        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            let result = handle
                .await
                .map_err(|e| Error::invalid_config(format!("Task failed: {e}")))?;
            results.push(result);
        }
        Ok(results)
    }

    fn validate_batch_body_file_args(operation: &BatchOperation) -> Result<(), Error> {
        let body_field_conflicts_with_args = operation.body_file.is_some()
            && operation.args.iter().any(|a| {
                a == "--body-file"
                    || a.starts_with("--body-file=")
                    || a == "--body"
                    || a.starts_with("--body=")
            });

        if body_field_conflicts_with_args {
            return Err(Error::invalid_config(
                "body_file field conflicts with --body or --body-file in args; use one or the other",
            ));
        }

        Ok(())
    }

    fn build_batch_cache_config(
        use_cache: Option<bool>,
    ) -> Result<Option<crate::response_cache::CacheConfig>, Error> {
        if !use_cache.unwrap_or(false) {
            return Ok(None);
        }

        let config_dir = if let Ok(dir) = std::env::var(crate::constants::ENV_APERTURE_CONFIG_DIR) {
            std::path::PathBuf::from(dir)
        } else {
            crate::config::manager::get_config_dir()?
        };

        Ok(Some(crate::response_cache::CacheConfig {
            cache_dir: config_dir
                .join(crate::constants::DIR_CACHE)
                .join(crate::constants::DIR_RESPONSES),
            default_ttl: std::time::Duration::from_secs(300),
            max_entries: 1000,
            enabled: true,
            allow_authenticated: false,
        }))
    }

    fn render_batch_execution_result(
        result: &crate::invocation::ExecutionResult,
        output_format: &crate::cli::OutputFormat,
        jq_filter: Option<&str>,
        suppress_output: bool,
        operation: &BatchOperation,
    ) -> Result<String, Error> {
        if suppress_output {
            let output =
                crate::cli::render::render_result_to_string(result, output_format, jq_filter)?;
            return Ok(output.unwrap_or_default());
        }

        crate::cli::render::render_result(result, output_format, jq_filter)?;

        Ok(format!(
            "Successfully executed operation: {}",
            operation
                .id
                .as_deref()
                .unwrap_or(crate::constants::DEFAULT_OPERATION_NAME)
        ))
    }

    /// Executes a single operation from a batch
    #[allow(clippy::too_many_arguments)]
    async fn execute_single_operation(
        spec: &CachedSpec,
        operation: &BatchOperation,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        output_format: &crate::cli::OutputFormat,
        jq_filter: Option<&str>,
        suppress_output: bool,
    ) -> Result<String, Error> {
        use crate::cli::translate;
        use crate::invocation::ExecutionContext;

        Self::validate_batch_body_file_args(operation)?;

        let command = generator::generate_command_tree_with_flags(spec, false);
        let extra_body_file: Vec<String> = operation
            .body_file
            .as_deref()
            .map(|p| vec!["--body-file".to_string(), p.to_string()])
            .unwrap_or_default();
        let matches = command
            .try_get_matches_from(
                std::iter::once(crate::constants::CLI_ROOT_COMMAND.to_string())
                    .chain(operation.args.clone())
                    .chain(extra_body_file),
            )
            .map_err(|e| Error::invalid_command(crate::constants::CONTEXT_BATCH, e.to_string()))?;

        let call = translate::matches_to_operation_call(spec, &matches)?;
        let cache_config = Self::build_batch_cache_config(operation.use_cache)?;
        let retry_context = build_batch_retry_context(operation, global_config)?;

        let ctx = ExecutionContext {
            dry_run,
            idempotency_key: None,
            cache_config,
            retry_context,
            base_url: base_url.map(String::from),
            global_config: global_config.cloned(),
            server_var_args: translate::extract_server_var_args(&matches),
            auto_paginate: false,
        };

        let result = crate::engine::executor::execute(spec, call, ctx).await?;

        Self::render_batch_execution_result(
            &result,
            output_format,
            jq_filter,
            suppress_output,
            operation,
        )
    }
}

/// Builds a `RetryContext` from batch operation settings and global configuration.
///
/// Operation-level settings take precedence over global config defaults.
#[allow(clippy::cast_possible_truncation)]
fn build_batch_retry_context(
    operation: &BatchOperation,
    global_config: Option<&GlobalConfig>,
) -> Result<Option<RetryContext>, Error> {
    let defaults = global_config.map(|c| &c.retry_defaults);
    let max_attempts = operation
        .retry
        .or_else(|| defaults.map(|d| d.max_attempts))
        .unwrap_or(0);

    if max_attempts == 0 {
        return Ok(None);
    }

    let initial_delay_ms = resolve_retry_delay_ms(
        operation.retry_delay.as_deref(),
        defaults.map_or(500, |d| d.initial_delay_ms),
    )?;
    let max_delay_ms = resolve_retry_delay_ms(
        operation.retry_max_delay.as_deref(),
        defaults.map_or(30_000, |d| d.max_delay_ms),
    )?;

    Ok(Some(RetryContext {
        max_attempts,
        initial_delay_ms,
        max_delay_ms,
        force_retry: operation.force_retry,
        method: None,               // Will be determined in executor
        has_idempotency_key: false, // Batch operations don't support idempotency keys yet
    }))
}

#[allow(clippy::cast_possible_truncation)]
fn resolve_retry_delay_ms(delay: Option<&str>, default_ms: u64) -> Result<u64, Error> {
    match delay {
        Some(delay_str) => Ok(parse_duration(delay_str)?.as_millis() as u64),
        None => Ok(default_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_parse_batch_file_json() {
        let batch_content = r#"{
            "metadata": {
                "name": "Test batch",
                "description": "A test batch file"
            },
            "operations": [
                {
                    "id": "op1",
                    "args": ["users", "list"],
                    "description": "List all users"
                },
                {
                    "id": "op2", 
                    "args": ["users", "get", "--user-id", "123"],
                    "description": "Get user 123"
                }
            ]
        }"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(batch_content.as_bytes()).unwrap();

        let batch_file = BatchProcessor::parse_batch_file(temp_file.path())
            .await
            .unwrap();

        assert_eq!(batch_file.operations.len(), 2);
        assert_eq!(batch_file.operations[0].args, vec!["users", "list"]);
        assert_eq!(
            batch_file.operations[1].args,
            vec!["users", "get", "--user-id", "123"]
        );
    }

    #[tokio::test]
    async fn test_parse_batch_file_yaml() {
        let batch_content = r#"
metadata:
  name: Test batch
  description: A test batch file
operations:
  - id: op1
    args: [users, list]
    description: List all users
  - id: op2
    args: [users, get, --user-id, "123"]
    description: Get user 123
"#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(batch_content.as_bytes()).unwrap();

        let batch_file = BatchProcessor::parse_batch_file(temp_file.path())
            .await
            .unwrap();

        assert_eq!(batch_file.operations.len(), 2);
        assert_eq!(batch_file.operations[0].args, vec!["users", "list"]);
        assert_eq!(
            batch_file.operations[1].args,
            vec!["users", "get", "--user-id", "123"]
        );
    }

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.max_concurrency, 5);
        assert_eq!(config.rate_limit, None);
        assert!(config.continue_on_error);
        assert!(config.show_progress);
    }

    #[test]
    fn test_batch_processor_creation() {
        let config = BatchConfig {
            max_concurrency: 10,
            rate_limit: Some(5),
            continue_on_error: false,
            show_progress: false,
            suppress_output: false,
        };

        let processor = BatchProcessor::new(config);
        assert_eq!(processor.semaphore.available_permits(), 10);
        assert!(processor.rate_limiter.is_some());
    }

    #[test]
    fn test_build_batch_retry_context_prefers_operation_values() {
        let operation = BatchOperation {
            retry: Some(3),
            retry_delay: Some("2s".to_string()),
            retry_max_delay: Some("5s".to_string()),
            force_retry: true,
            ..Default::default()
        };

        let mut global_config = GlobalConfig::default();
        global_config.retry_defaults.max_attempts = 7;
        global_config.retry_defaults.initial_delay_ms = 1_000;
        global_config.retry_defaults.max_delay_ms = 10_000;

        let retry_context = build_batch_retry_context(&operation, Some(&global_config))
            .expect("retry context should build")
            .expect("retry should be enabled");

        assert_eq!(retry_context.max_attempts, 3);
        assert_eq!(retry_context.initial_delay_ms, 2_000);
        assert_eq!(retry_context.max_delay_ms, 5_000);
        assert!(retry_context.force_retry);
        assert!(!retry_context.has_idempotency_key);
    }

    #[test]
    fn test_build_batch_retry_context_uses_global_defaults() {
        let operation = BatchOperation {
            retry: None,
            retry_delay: None,
            retry_max_delay: None,
            force_retry: false,
            ..Default::default()
        };

        let mut global_config = GlobalConfig::default();
        global_config.retry_defaults.max_attempts = 4;
        global_config.retry_defaults.initial_delay_ms = 750;
        global_config.retry_defaults.max_delay_ms = 5_500;

        let retry_context = build_batch_retry_context(&operation, Some(&global_config))
            .expect("retry context should build")
            .expect("retry should be enabled");

        assert_eq!(retry_context.max_attempts, 4);
        assert_eq!(retry_context.initial_delay_ms, 750);
        assert_eq!(retry_context.max_delay_ms, 5_500);
        assert!(!retry_context.force_retry);
        assert!(!retry_context.has_idempotency_key);
    }

    #[test]
    fn test_build_batch_retry_context_disables_when_attempts_are_zero() {
        let operation = BatchOperation::default();
        let global_config = GlobalConfig::default();

        assert!(build_batch_retry_context(&operation, Some(&global_config))
            .expect("retry context should build")
            .is_none());
    }
}
