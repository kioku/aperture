//! CLI-agnostic invocation model for the execution engine.
//!
//! These types decouple the execution core from any specific CLI framework
//! (e.g., clap). The executor accepts [`OperationCall`] and [`ExecutionContext`]
//! and returns [`ExecutionResult`], enabling library/SDK usage, alternative
//! frontends, and unit testing without CLI parsing dependencies.

use crate::config::models::GlobalConfig;
use crate::engine::executor::RetryContext;
use crate::response_cache::CacheConfig;
use serde_json::Value;
use std::collections::HashMap;

/// Describes a single API operation to invoke, fully resolved from user input.
///
/// All parameter values are pre-extracted and categorized by their `OpenAPI`
/// location (path, query, header). This struct is framework-agnostic — it
/// can be constructed from clap `ArgMatches`, a GUI form, or programmatically.
#[derive(Debug, Clone)]
pub struct OperationCall {
    /// The `operationId` from the `OpenAPI` spec (e.g., `"getUserById"`).
    pub operation_id: String,

    /// Path parameters keyed by name (e.g., `{"id": "123"}`).
    pub path_params: HashMap<String, String>,

    /// Query parameters keyed by name (e.g., `{"page": "1"}`).
    pub query_params: HashMap<String, String>,

    /// Header parameters keyed by name (e.g., `{"X-Request-Id": "abc"}`).
    pub header_params: HashMap<String, String>,

    /// Optional JSON request body.
    pub body: Option<String>,

    /// Custom headers in raw `"Name: Value"` format, as provided by the user.
    pub custom_headers: Vec<String>,
}

/// Execution-time configuration that is orthogonal to the operation itself.
///
/// Controls retry behavior, caching, dry-run mode, and authentication
/// context. Does **not** include rendering concerns (output format, JQ
/// filters) — those belong in the CLI rendering layer.
#[derive(Debug, Clone, Default)]
pub struct ExecutionContext {
    /// If true, show the request that would be made without executing it.
    pub dry_run: bool,

    /// Optional idempotency key for safe retries.
    pub idempotency_key: Option<String>,

    /// Response cache configuration. `None` disables caching.
    pub cache_config: Option<CacheConfig>,

    /// Retry configuration. `None` disables retries.
    pub retry_context: Option<RetryContext>,

    /// Base URL override. `None` uses `BaseUrlResolver` priority chain.
    pub base_url: Option<String>,

    /// Global configuration for URL resolution and secret lookup.
    pub global_config: Option<GlobalConfig>,

    /// Server template variable overrides (e.g., `["region=us", "env=prod"]`).
    pub server_var_args: Vec<String>,
}

/// Structured result returned by the executor. The CLI layer decides how
/// to render this (JSON, YAML, table, etc.) — the executor never prints.
#[derive(Debug, Clone)]
pub enum ExecutionResult {
    /// Successful HTTP response.
    Success {
        /// Response body text.
        body: String,
        /// HTTP status code.
        status: u16,
        /// Response headers.
        headers: HashMap<String, String>,
    },

    /// Dry-run mode: the request that *would* have been sent.
    DryRun {
        /// Structured JSON representation of the request.
        request_info: Value,
    },

    /// Response served from cache.
    Cached {
        /// Cached response body text.
        body: String,
    },

    /// The operation completed but produced no response body.
    Empty,
}
