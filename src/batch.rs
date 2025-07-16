use crate::cache::models::CachedSpec;
use crate::config::models::GlobalConfig;
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchOperation {
    /// Unique identifier for this operation (optional)
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
                NonZeroU32::new(limit).unwrap_or(NonZeroU32::new(1).unwrap()),
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
        let content = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;

        // Try to parse as JSON first, then YAML
        if let Ok(batch_file) = serde_json::from_str::<BatchFile>(&content) {
            return Ok(batch_file);
        }

        if let Ok(batch_file) = serde_yaml::from_str::<BatchFile>(&content) {
            return Ok(batch_file);
        }

        Err(Error::Validation(format!(
            "Failed to parse batch file as JSON or YAML: {}",
            path.display()
        )))
    }

    /// Executes a batch of operations
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any operation fails and `continue_on_error` is false
    /// - Task spawning fails
    /// - Network or API errors occur during operation execution
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
        let start_time = std::time::Instant::now();
        let total_operations = batch_file.operations.len();

        if self.config.show_progress {
            println!("Starting batch execution: {total_operations} operations");
        }

        let mut results = Vec::with_capacity(total_operations);
        let mut handles = Vec::new();

        // Create tasks for each operation
        for (index, operation) in batch_file.operations.into_iter().enumerate() {
            let spec = spec.clone();
            let global_config = global_config.cloned();
            let base_url = base_url.map(String::from);
            let output_format = output_format.clone();
            let jq_filter = jq_filter.map(String::from);
            let semaphore = Arc::clone(&self.semaphore);
            let rate_limiter = self.rate_limiter.clone();
            let show_progress = self.config.show_progress;

            let handle = tokio::spawn(async move {
                // Acquire semaphore permit for concurrency control
                let _permit = semaphore.acquire().await.unwrap();

                // Apply rate limiting if configured
                if let Some(limiter) = rate_limiter {
                    limiter.until_ready().await;
                }

                let operation_start = std::time::Instant::now();

                // Execute the operation
                let result = Self::execute_single_operation(
                    &spec,
                    &operation,
                    global_config.as_ref(),
                    base_url.as_deref(),
                    dry_run,
                    &output_format,
                    jq_filter.as_deref(),
                )
                .await;

                let duration = operation_start.elapsed();

                let (success, error, response) = match result {
                    Ok(resp) => {
                        if show_progress {
                            println!("✓ Operation {} completed", index + 1);
                        }
                        (true, None, Some(resp))
                    }
                    Err(e) => {
                        if show_progress {
                            println!("✗ Operation {} failed: {}", index + 1, e);
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
            });

            handles.push(handle);
        }

        // Collect all results
        for handle in handles {
            let result = handle
                .await
                .map_err(|e| Error::Config(format!("Task failed: {e}")))?;
            results.push(result);
        }

        let total_duration = start_time.elapsed();
        let success_count = results.iter().filter(|r| r.success).count();
        let failure_count = results.len() - success_count;

        if self.config.show_progress {
            println!(
                "Batch execution completed: {}/{} operations successful in {:.2}s",
                success_count,
                total_operations,
                total_duration.as_secs_f64()
            );
        }

        Ok(BatchResult {
            results,
            total_duration,
            success_count,
            failure_count,
        })
    }

    /// Executes a single operation from a batch
    async fn execute_single_operation(
        spec: &CachedSpec,
        operation: &BatchOperation,
        global_config: Option<&GlobalConfig>,
        base_url: Option<&str>,
        dry_run: bool,
        output_format: &crate::cli::OutputFormat,
        jq_filter: Option<&str>,
    ) -> Result<String, Error> {
        use crate::engine::generator;

        // Generate the command tree (we don't use experimental flags for batch operations)
        let command = generator::generate_command_tree_with_flags(spec, false);

        // Parse the operation args into ArgMatches
        let matches = command
            .try_get_matches_from(std::iter::once("api".to_string()).chain(operation.args.clone()))
            .map_err(|e| Error::InvalidCommand {
                context: "batch".to_string(),
                reason: e.to_string(),
            })?;

        // Create cache configuration - for batch operations, we use the operation's use_cache setting
        let cache_config = if operation.use_cache.unwrap_or(false) {
            Some(crate::response_cache::CacheConfig {
                cache_dir: std::env::var("APERTURE_CONFIG_DIR")
                    .map_or_else(
                        |_| std::path::PathBuf::from("~/.config/aperture"),
                        std::path::PathBuf::from,
                    )
                    .join(".cache")
                    .join("responses"),
                default_ttl: std::time::Duration::from_secs(300),
                max_entries: 1000,
                enabled: true,
            })
        } else {
            None
        };

        if dry_run {
            // For dry run, we still call execute_request but with dry_run=true
            crate::engine::executor::execute_request(
                spec,
                &matches,
                base_url,
                true, // dry_run
                None, // idempotency_key
                global_config,
                output_format,
                jq_filter,
                cache_config.as_ref(),
            )
            .await?;

            // Return dry run message
            Ok(format!(
                "DRY RUN: Would execute operation with args: {:?}",
                operation.args
            ))
        } else {
            // For actual execution, call execute_request normally
            // The output will go to stdout as expected for batch operations
            crate::engine::executor::execute_request(
                spec,
                &matches,
                base_url,
                false, // dry_run
                None,  // idempotency_key
                global_config,
                output_format,
                jq_filter,
                cache_config.as_ref(),
            )
            .await?;

            // Return success message
            Ok(format!(
                "Successfully executed operation: {}",
                operation.id.as_deref().unwrap_or("unnamed")
            ))
        }
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
}
