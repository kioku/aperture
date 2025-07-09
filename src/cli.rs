use clap::{Parser, Subcommand, ValueEnum};

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
    #[arg(long, global = true, help = "Output capability manifest as JSON")]
    pub describe_json: bool,

    /// Output all errors as structured JSON to stderr
    #[arg(long, global = true, help = "Output errors in JSON format")]
    pub json_errors: bool,

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

    /// Apply JQ filter to response data
    #[arg(
        long,
        global = true,
        value_name = "FILTER",
        help = "Apply JQ filter to response data (e.g., '.name', '.[] | select(.active)')"
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
        /// Name of the API specification context
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
        /// Name of the API specification context
        context: String,
        /// Remaining arguments will be parsed dynamically based on the `OpenAPI` spec
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
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
        /// Name to identify this API specification (used as context in 'aperture api')
        name: String,
        /// Path to the `OpenAPI` 3.x specification file (YAML format) or URL
        file_or_url: String,
        /// Overwrite existing specification if it already exists
        #[arg(long, help = "Replace the specification if it already exists")]
        force: bool,
    },
    /// List all registered API specifications
    #[command(
        long_about = "Display all currently registered API specifications.\n\n\
                      Shows the names you can use as contexts with 'aperture api'.\n\
                      Use this to see what APIs are available for command generation."
    )]
    List {},
    /// Remove an API specification from configuration
    #[command(
        long_about = "Remove a registered API specification and its cached data.\n\n\
                      This removes both the original specification file and the\n\
                      generated cache, making the API operations unavailable.\n\
                      Use 'aperture config list' to see available specifications."
    )]
    Remove {
        /// Name of the API specification to remove
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
        /// Name of the API specification to edit
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
        /// Name of the API specification
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
        /// Name of the API specification
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
        /// Name of the API specification to reinitialize (omit for --all)
        context: Option<String>,
        /// Reinitialize all cached specifications
        #[arg(long, conflicts_with = "context", help = "Reinitialize all specs")]
        all: bool,
    },
}
