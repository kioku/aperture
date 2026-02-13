pub mod commands;
pub mod errors;
pub mod legacy_execute;
pub mod render;
pub mod tracing_init;
pub mod translate;

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(ValueEnum, Clone, Debug)]
pub enum OutputFormat {
    /// Output as JSON (default)
    Json,
    /// Output as YAML
    Yaml,
    /// Output as formatted table
    Table,
}

#[derive(Parser, Debug)]
#[allow(clippy::struct_excessive_bools)]
#[command(
    author,
    version,
    about = "Aperture: Dynamic CLI generator for OpenAPI specifications",
    long_about = "Aperture dynamically generates commands from OpenAPI 3.x specifications.\n\
                  It serves as a bridge between autonomous AI agents and APIs by consuming\n\
                  OpenAPI specs and creating a rich command-line interface with built-in\n\
                  security, caching, and agent-friendly features.\n\n\
                  Examples:\n  \
                  aperture config add myapi api-spec.yaml\n  \
                  aperture api myapi users get-user --id 123\n  \
                  aperture config list\n\n\
                  Agent-friendly features:\n  \
                  aperture api myapi --describe-json    # Get capability manifest\n  \
                  aperture --json-errors api myapi ...  # Structured error output\n  \
                  aperture api myapi --dry-run ...      # Show request without executing"
)]
pub struct Cli {
    /// Output a JSON manifest of all available commands and parameters
    #[arg(
        long,
        global = true,
        help = "Output capability manifest as JSON (can be filtered with --jq)"
    )]
    pub describe_json: bool,

    /// Output all errors as structured JSON to stderr
    /// When used with batch operations, outputs a clean JSON summary at the end
    #[arg(long, global = true, help = "Output errors in JSON format")]
    pub json_errors: bool,

    /// Suppress non-essential output (success messages, tips, hints)
    /// Only outputs requested data and errors
    #[arg(
        long,
        short = 'q',
        global = true,
        help = "Suppress informational output"
    )]
    pub quiet: bool,

    /// Increase logging verbosity
    #[arg(
        short = 'v',
        global = true,
        action = ArgAction::Count,
        help = "Increase logging verbosity (-v for debug, -vv for trace)"
    )]
    pub verbosity: u8,

    /// Show the HTTP request that would be made without executing it
    #[arg(long, global = true, help = "Show request details without executing")]
    pub dry_run: bool,

    /// Set the Idempotency-Key header for safe retries
    #[arg(
        long,
        global = true,
        value_name = "KEY",
        help = "Set idempotency key header"
    )]
    pub idempotency_key: Option<String>,

    /// Output format for response data
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "json",
        help = "Output format for response data"
    )]
    pub format: OutputFormat,

    /// Apply JQ filter to response data, describe-json output, or batch results (with --json-errors)
    #[arg(
        long,
        global = true,
        value_name = "FILTER",
        help = "Apply JQ filter to JSON output (e.g., '.name', '.[] | select(.active)', '.batch_execution_summary.operations[] | select(.success == false)')"
    )]
    pub jq: Option<String>,

    /// Execute operations from a batch file
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Path to batch file (JSON or YAML) containing multiple operations"
    )]
    pub batch_file: Option<String>,

    /// Maximum concurrent requests for batch operations
    #[arg(
        long,
        global = true,
        value_name = "N",
        default_value = "5",
        help = "Maximum number of concurrent requests for batch operations"
    )]
    pub batch_concurrency: usize,

    /// Rate limit for batch operations (requests per second)
    #[arg(
        long,
        global = true,
        value_name = "N",
        help = "Rate limit for batch operations (requests per second)"
    )]
    pub batch_rate_limit: Option<u32>,

    /// Enable response caching
    #[arg(
        long,
        global = true,
        help = "Enable response caching (can speed up repeated requests)"
    )]
    pub cache: bool,

    /// Disable response caching
    #[arg(
        long,
        global = true,
        conflicts_with = "cache",
        help = "Disable response caching"
    )]
    pub no_cache: bool,

    /// TTL for cached responses in seconds
    #[arg(
        long,
        global = true,
        value_name = "SECONDS",
        help = "Cache TTL in seconds (default: 300)"
    )]
    pub cache_ttl: Option<u64>,

    /// Use positional arguments for path parameters (legacy syntax)
    #[arg(
        long,
        global = true,
        help = "Use positional arguments for path parameters (legacy syntax)"
    )]
    pub positional_args: bool,

    /// Maximum number of retry attempts for failed requests
    #[arg(
        long,
        global = true,
        value_name = "N",
        help = "Maximum retry attempts (0 = disabled, overrides config)"
    )]
    pub retry: Option<u32>,

    /// Initial delay between retries (e.g., "500ms", "1s")
    #[arg(
        long,
        global = true,
        value_name = "DURATION",
        help = "Initial retry delay (e.g., '500ms', '1s', '2s')"
    )]
    pub retry_delay: Option<String>,

    /// Maximum delay cap between retries (e.g., "30s", "1m")
    #[arg(
        long,
        global = true,
        value_name = "DURATION",
        help = "Maximum retry delay cap (e.g., '30s', '1m')"
    )]
    pub retry_max_delay: Option<String>,

    /// Force retry on non-idempotent requests without an idempotency key
    #[arg(
        long,
        global = true,
        help = "Allow retrying non-idempotent requests without idempotency key"
    )]
    pub force_retry: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage API specifications (add, list, remove, edit)
    #[command(long_about = "Manage your collection of OpenAPI specifications.\n\n\
                      Add specifications to make their operations available as commands,\n\
                      list currently registered specs, remove unused ones, or edit\n\
                      existing specifications in your default editor.")]
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// List available commands for an API specification
    #[command(
        long_about = "Display a tree-like summary of all available commands for an API.\n\n\
                      Shows operations organized by tags, making it easy to discover\n\
                      what functionality is available in a registered API specification.\n\
                      This provides an overview without having to use --help on each operation.\n\n\
                      Example:\n  \
                      aperture list-commands myapi"
    )]
    ListCommands {
        /// Name of the API specification context.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        context: String,
    },
    /// Execute API operations for a specific context
    #[command(
        long_about = "Execute operations from a registered API specification.\n\n\
                      The context refers to the name you gave when adding the spec.\n\
                      Commands are dynamically generated based on the OpenAPI specification,\n\
                      organized by tags (e.g., 'users', 'posts', 'orders').\n\n\
                      Examples:\n  \
                      aperture api myapi users get-user --id 123\n  \
                      aperture api myapi posts create-post --body '{\"title\":\"Hello\"}'\n  \
                      aperture api myapi --help  # See available operations"
    )]
    Api {
        /// Name of the API specification context.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        context: String,
        /// Remaining arguments will be parsed dynamically based on the `OpenAPI` spec
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Search for API operations across all specifications
    #[command(long_about = "Search for API operations by keyword or pattern.\n\n\
                      Search through all registered API specifications to find\n\
                      relevant operations. The search includes operation IDs,\n\
                      descriptions, paths, and HTTP methods.\n\n\
                      Examples:\n  \
                      aperture search 'list users'     # Find user listing operations\n  \
                      aperture search 'POST create'     # Find POST operations with 'create'\n  \
                      aperture search issues --api sm   # Search only in 'sm' API\n  \
                      aperture search 'get.*by.*id'     # Regex pattern search")]
    Search {
        /// Search query (keywords, patterns, or regex)
        query: String,
        /// Limit search to a specific API context.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        #[arg(long, value_name = "API", help = "Search only in specified API")]
        api: Option<String>,
        /// Show detailed results including paths and parameters
        #[arg(long, help = "Show detailed information for each result")]
        verbose: bool,
    },
    /// Execute API operations using shortcuts or direct operation IDs
    #[command(
        name = "exec",
        long_about = "Execute API operations using shortcuts instead of full paths.\n\n\
                      This command attempts to resolve shortcuts to their full command paths:\n\
                      - Direct operation IDs: getUserById --id 123\n\
                      - HTTP method + path: GET /users/123\n\
                      - Tag-based shortcuts: users list\n\n\
                      When multiple matches are found, you'll get suggestions to choose from.\n\n\
                      Examples:\n  \
                      aperture exec getUserById --id 123\n  \
                      aperture exec GET /users/123\n  \
                      aperture exec users list"
    )]
    Exec {
        /// Shortcut command arguments
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Get detailed documentation for APIs and commands
    #[command(
        long_about = "Get comprehensive documentation for APIs and operations.\n\n\
                      This provides detailed information including parameters, examples,\n\
                      response schemas, and authentication requirements. Use it to learn\n\
                      about available functionality without trial and error.\n\n\
                      Examples:\n  \
                      aperture docs                        # Interactive help menu\n  \
                      aperture docs myapi                  # API overview\n  \
                      aperture docs myapi users get-user  # Detailed command help"
    )]
    Docs {
        /// API name (optional, shows interactive menu if omitted).
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api: Option<String>,
        /// Tag/category name (optional)
        tag: Option<String>,
        /// Operation name (optional)
        operation: Option<String>,
        /// Show enhanced formatting with examples
        #[arg(long, help = "Enhanced formatting with examples and tips")]
        enhanced: bool,
    },
    /// Show API overview with statistics and quick start guide
    #[command(
        long_about = "Display comprehensive API overview with statistics and examples.\n\n\
                      Shows operation counts, method distribution, available categories,\n\
                      and sample commands to help you get started quickly with any API.\n\n\
                      Examples:\n  \
                      aperture overview myapi\n  \
                      aperture overview --all  # Overview of all registered APIs"
    )]
    Overview {
        /// API name (required unless using --all).
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api: Option<String>,
        /// Show overview for all registered APIs
        #[arg(long, conflicts_with = "api", help = "Show overview for all APIs")]
        all: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommands {
    /// Add a new API specification from a file
    #[command(
        long_about = "Add an OpenAPI 3.x specification to your configuration.\n\n\
                      This validates the specification, extracts operations, and creates\n\
                      a cached representation for fast command generation. The spec name\n\
                      becomes the context for executing API operations.\n\n\
                      Supported formats: YAML (.yaml, .yml)\n\
                      Supported auth: API Key, Bearer Token\n\n\
                      Examples:\n  \
                      aperture config add myapi ./openapi.yaml\n  \
                      aperture config add myapi https://api.example.com/openapi.yaml"
    )]
    Add {
        /// Name to identify this API specification (used as context in 'aperture api').
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        name: String,
        /// Path to the `OpenAPI` 3.x specification file (YAML format) or URL
        file_or_url: String,
        /// Overwrite existing specification if it already exists
        #[arg(long, help = "Replace the specification if it already exists")]
        force: bool,
        /// Reject specs with unsupported features instead of skipping endpoints
        #[arg(
            long,
            help = "Reject entire spec if any endpoints have unsupported content types (e.g., multipart/form-data, XML). Default behavior skips unsupported endpoints with warnings."
        )]
        strict: bool,
    },
    /// List all registered API specifications
    #[command(
        long_about = "Display all currently registered API specifications.\n\n\
                      Shows the names you can use as contexts with 'aperture api'.\n\
                      Use this to see what APIs are available for command generation."
    )]
    List {
        /// Show detailed information including skipped endpoints
        #[arg(long, help = "Show detailed information about each API")]
        verbose: bool,
    },
    /// Remove an API specification from configuration
    #[command(
        long_about = "Remove a registered API specification and its cached data.\n\n\
                      This removes both the original specification file and the\n\
                      generated cache, making the API operations unavailable.\n\
                      Use 'aperture config list' to see available specifications."
    )]
    Remove {
        /// Name of the API specification to remove.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        name: String,
    },
    /// Edit an API specification in your default editor
    #[command(
        long_about = "Open an API specification in your default text editor.\n\n\
                      Uses the $EDITOR environment variable to determine which editor\n\
                      to use. After editing, you may need to re-add the specification\n\
                      to update the cached representation.\n\n\
                      Example:\n  \
                      export EDITOR=vim\n  \
                      aperture config edit myapi"
    )]
    Edit {
        /// Name of the API specification to edit.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        name: String,
    },
    /// Set base URL for an API specification
    #[command(long_about = "Set the base URL for an API specification.\n\n\
                      This overrides the base URL from the OpenAPI spec and the\n\
                      APERTURE_BASE_URL environment variable. You can set a general\n\
                      override or environment-specific URLs.\n\n\
                      Examples:\n  \
                      aperture config set-url myapi https://api.example.com\n  \
                      aperture config set-url myapi --env staging https://staging.example.com\n  \
                      aperture config set-url myapi --env prod https://prod.example.com")]
    SetUrl {
        /// Name of the API specification.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        name: String,
        /// The base URL to set
        url: String,
        /// Set URL for a specific environment (e.g., dev, staging, prod)
        #[arg(long, value_name = "ENV", help = "Set URL for specific environment")]
        env: Option<String>,
    },
    /// Get base URL configuration for an API specification
    #[command(
        long_about = "Display the base URL configuration for an API specification.\n\n\
                      Shows the configured base URL override and any environment-specific\n\
                      URLs. Also displays what URL would be used based on current\n\
                      environment settings.\n\n\
                      Example:\n  \
                      aperture config get-url myapi"
    )]
    GetUrl {
        /// Name of the API specification.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        name: String,
    },
    /// List all configured base URLs
    #[command(
        long_about = "Display all configured base URLs across all API specifications.\n\n\
                      Shows general overrides and environment-specific configurations\n\
                      for each registered API. Useful for reviewing your URL settings\n\
                      at a glance."
    )]
    ListUrls {},
    /// Set secret configuration for an API specification security scheme
    #[command(
        long_about = "Configure authentication secrets for API specifications.\n\n\
                      This allows you to set environment variable mappings for security\n\
                      schemes without modifying the OpenAPI specification file. These\n\
                      settings take precedence over x-aperture-secret extensions.\n\n\
                      Examples:\n  \
                      aperture config set-secret myapi bearerAuth --env API_TOKEN\n  \
                      aperture config set-secret myapi apiKey --env API_KEY\n  \
                      aperture config set-secret myapi --interactive"
    )]
    SetSecret {
        /// Name of the API specification.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api_name: String,
        /// Name of the security scheme (omit for interactive mode)
        scheme_name: Option<String>,
        /// Environment variable name containing the secret
        #[arg(long, value_name = "VAR", help = "Environment variable name")]
        env: Option<String>,
        /// Interactive mode to configure all undefined secrets
        #[arg(long, conflicts_with_all = ["scheme_name", "env"], help = "Configure secrets interactively")]
        interactive: bool,
    },
    /// List configured secrets for an API specification
    #[command(
        long_about = "Display configured secret mappings for an API specification.\n\n\
                      Shows which security schemes are configured with environment\n\
                      variables and which ones still rely on x-aperture-secret\n\
                      extensions or are undefined.\n\n\
                      Example:\n  \
                      aperture config list-secrets myapi"
    )]
    ListSecrets {
        /// Name of the API specification.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api_name: String,
    },
    /// Remove a specific configured secret for an API specification
    #[command(
        long_about = "Remove a configured secret mapping for a specific security scheme.\n\n\
                      This will remove the environment variable mapping for the specified\n\
                      security scheme, causing it to fall back to x-aperture-secret\n\
                      extensions or become undefined.\n\n\
                      Examples:\n  \
                      aperture config remove-secret myapi bearerAuth\n  \
                      aperture config remove-secret myapi apiKey"
    )]
    RemoveSecret {
        /// Name of the API specification.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api_name: String,
        /// Name of the security scheme to remove
        scheme_name: String,
    },
    /// Clear all configured secrets for an API specification
    #[command(
        long_about = "Remove all configured secret mappings for an API specification.\n\n\
                      This will remove all environment variable mappings for the API,\n\
                      causing all security schemes to fall back to x-aperture-secret\n\
                      extensions or become undefined. Use with caution.\n\n\
                      Examples:\n  \
                      aperture config clear-secrets myapi\n  \
                      aperture config clear-secrets myapi --force"
    )]
    ClearSecrets {
        /// Name of the API specification.
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api_name: String,
        /// Skip confirmation prompt
        #[arg(long, help = "Skip confirmation prompt")]
        force: bool,
    },
    /// Re-initialize cached specifications
    #[command(
        long_about = "Regenerate binary cache files for API specifications.\n\n\
                      This is useful when cache files become corrupted or when upgrading\n\
                      between versions of Aperture that have incompatible cache formats.\n\
                      You can reinitialize all specs or target a specific one.\n\n\
                      Examples:\n  \
                      aperture config reinit --all     # Reinitialize all specs\n  \
                      aperture config reinit myapi     # Reinitialize specific spec"
    )]
    Reinit {
        /// Name of the API specification to reinitialize (omit for --all).
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        context: Option<String>,
        /// Reinitialize all cached specifications
        #[arg(long, conflicts_with = "context", help = "Reinitialize all specs")]
        all: bool,
    },
    /// Clear response cache
    #[command(long_about = "Clear cached API responses to free up disk space.\n\n\
                      You can clear cache for a specific API or all cached responses.\n\
                      This is useful when you want to ensure fresh data from the API\n\
                      or free up disk space.\n\n\
                      Examples:\n  \
                      aperture config clear-cache myapi     # Clear cache for specific API\n  \
                      aperture config clear-cache --all     # Clear all cached responses")]
    ClearCache {
        /// Name of the API specification to clear cache for (omit for --all).
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api_name: Option<String>,
        /// Clear all cached responses
        #[arg(long, conflicts_with = "api_name", help = "Clear all response cache")]
        all: bool,
    },
    /// Show response cache statistics
    #[command(long_about = "Display statistics about cached API responses.\n\n\
                      Shows cache size, number of entries, and hit/miss rates.\n\
                      Useful for monitoring cache effectiveness and disk usage.\n\n\
                      Examples:\n  \
                      aperture config cache-stats myapi     # Stats for specific API\n  \
                      aperture config cache-stats           # Stats for all APIs")]
    CacheStats {
        /// Name of the API specification to show stats for (omit for all APIs).
        /// Must start with a letter or digit; may contain letters, digits, dots, hyphens, or underscores (max 64 chars).
        api_name: Option<String>,
    },
    /// Set a global configuration setting
    #[command(long_about = "Set a global configuration setting value.\n\n\
                      Supports dot-notation for nested settings and type-safe validation.\n\
                      The configuration file comments and formatting are preserved.\n\n\
                      Available settings:\n  \
                      default_timeout_secs              (integer)  - Default timeout for API requests\n  \
                      agent_defaults.json_errors        (boolean)  - Output errors as JSON by default\n  \
                      retry_defaults.max_attempts       (integer)  - Max retry attempts (0 = disabled)\n  \
                      retry_defaults.initial_delay_ms   (integer)  - Initial retry delay in ms\n  \
                      retry_defaults.max_delay_ms       (integer)  - Maximum retry delay cap in ms\n\n\
                      Examples:\n  \
                      aperture config set default_timeout_secs 60\n  \
                      aperture config set agent_defaults.json_errors true\n  \
                      aperture config set retry_defaults.max_attempts 3")]
    Set {
        /// Setting key (use `config settings` to see all available keys)
        key: String,
        /// Value to set (validated against expected type)
        value: String,
    },
    /// Get a global configuration setting value
    #[command(
        long_about = "Get the current value of a global configuration setting.\n\n\
                      Supports dot-notation for nested settings.\n\
                      Use `config settings` to see all available keys.\n\n\
                      Examples:\n  \
                      aperture config get default_timeout_secs\n  \
                      aperture config get retry_defaults.max_attempts\n  \
                      aperture config get default_timeout_secs --json"
    )]
    Get {
        /// Setting key to retrieve (use `config settings` to see all available keys)
        key: String,
        /// Output as JSON
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
    /// List all available configuration settings
    #[command(
        long_about = "Display all available configuration settings and their current values.\n\n\
                      Shows the key name, current value, type, and description for each\n\
                      setting. Use this to discover available settings and their defaults.\n\n\
                      Examples:\n  \
                      aperture config settings\n  \
                      aperture config settings --json"
    )]
    Settings {
        /// Output as JSON
        #[arg(long, help = "Output as JSON")]
        json: bool,
    },
}
