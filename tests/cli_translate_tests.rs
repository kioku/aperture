use aperture_cli::cache::models::{
    CachedCommand, CachedParameter, CachedRequestBody, CachedResponse, CachedSpec,
};
use aperture_cli::cli::translate::{
    cli_to_execution_context, extract_server_var_args, has_show_examples_flag,
    matches_to_operation_call, matches_to_operation_id,
};
use aperture_cli::cli::{Cli, Commands, OutputFormat};
use aperture_cli::config::models::GlobalConfig;
use clap::{Arg, ArgAction, Command};
use std::collections::HashMap;

fn cached_parameter(
    name: &str,
    location: &str,
    schema_type: &str,
    required: bool,
) -> CachedParameter {
    CachedParameter {
        name: name.to_string(),
        location: location.to_string(),
        required,
        description: None,
        schema: Some(format!("{{\"type\":\"{schema_type}\"}}")),
        schema_type: Some(schema_type.to_string()),
        format: None,
        default_value: None,
        enum_values: vec![],
        example: None,
    }
}

fn build_spec(parameters: Vec<CachedParameter>, with_body: bool) -> CachedSpec {
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![CachedCommand {
            name: "users".to_string(),
            description: None,
            summary: None,
            operation_id: "getUserById".to_string(),
            method: "GET".to_string(),
            path: "/users/{id}".to_string(),
            parameters,
            request_body: with_body.then_some(CachedRequestBody {
                content_type: "application/json".to_string(),
                schema: "{\"type\":\"object\"}".to_string(),
                required: false,
                description: None,
                example: None,
            }),
            responses: vec![CachedResponse {
                status_code: "200".to_string(),
                description: Some("ok".to_string()),
                content_type: Some("application/json".to_string()),
                schema: None,
                example: None,
            }],
            security_requirements: vec![],
            tags: vec!["users".to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
        }],
        base_url: Some("https://api.example.com".to_string()),
        servers: vec!["https://api.example.com".to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

fn build_matches(include_show_examples: bool) -> clap::ArgMatches {
    let mut operation_cmd = Command::new("get-user-by-id")
        .arg(Arg::new("id").required(true))
        .arg(Arg::new("limit").long("limit"))
        .arg(
            Arg::new("verbose")
                .long("verbose")
                .action(ArgAction::SetTrue),
        )
        .arg(Arg::new("request_id").long("request-id"))
        .arg(Arg::new("body").long("body"))
        .arg(
            Arg::new("header")
                .short('H')
                .long("header")
                .action(ArgAction::Append),
        )
        .arg(
            Arg::new("server-var")
                .long("server-var")
                .action(ArgAction::Append),
        );

    if include_show_examples {
        operation_cmd = operation_cmd.arg(
            Arg::new("show-examples")
                .long("show-examples")
                .action(ArgAction::SetTrue),
        );
    }

    Command::new("aperture")
        .subcommand(Command::new("users").subcommand(operation_cmd))
        .get_matches_from(vec![
            "aperture",
            "users",
            "get-user-by-id",
            "123",
            "--limit",
            "10",
            "--verbose",
            "--request-id",
            "req-1",
            "-H",
            "X-Custom: abc",
            "--server-var",
            "region=us",
            "--show-examples",
        ])
}

#[allow(clippy::missing_const_for_fn)]
fn base_cli() -> Cli {
    Cli {
        describe_json: false,
        json_errors: false,
        quiet: false,
        verbosity: 0,
        dry_run: false,
        idempotency_key: None,
        format: OutputFormat::Json,
        jq: None,
        batch_file: None,
        batch_concurrency: 5,
        batch_rate_limit: None,
        cache: false,
        no_cache: false,
        cache_ttl: None,
        positional_args: false,
        retry: None,
        retry_delay: None,
        retry_max_delay: None,
        force_retry: false,
        command: Commands::Exec { args: vec![] },
    }
}

#[test]
fn matches_to_operation_call_extracts_all_parameter_types() {
    let spec = build_spec(
        vec![
            cached_parameter("id", "path", "string", true),
            cached_parameter("limit", "query", "string", false),
            cached_parameter("verbose", "query", "boolean", false),
            cached_parameter("request_id", "header", "string", false),
        ],
        false,
    );

    let matches = build_matches(true);
    let call = matches_to_operation_call(&spec, &matches).expect("translation should succeed");

    assert_eq!(call.operation_id, "getUserById");
    assert_eq!(call.path_params.get("id"), Some(&"123".to_string()));
    assert_eq!(call.query_params.get("limit"), Some(&"10".to_string()));
    assert_eq!(call.query_params.get("verbose"), Some(&"true".to_string()));
    assert_eq!(
        call.header_params.get("request_id"),
        Some(&"req-1".to_string())
    );
    assert_eq!(call.custom_headers, vec!["X-Custom: abc".to_string()]);
}

#[test]
fn matches_to_operation_call_does_not_panic_on_non_boolean_parameters() {
    let spec = build_spec(
        vec![cached_parameter("limit", "query", "string", false)],
        false,
    );

    let matches = Command::new("aperture")
        .subcommand(
            Command::new("users")
                .subcommand(Command::new("get-user-by-id").arg(Arg::new("limit").long("limit"))),
        )
        .get_matches_from(vec!["aperture", "users", "get-user-by-id", "--limit", "25"]);

    let call = matches_to_operation_call(&spec, &matches).expect("translation should succeed");
    assert_eq!(call.query_params.get("limit"), Some(&"25".to_string()));
}

#[test]
fn matches_to_operation_call_rejects_invalid_json_body() {
    let spec = build_spec(vec![cached_parameter("id", "path", "string", true)], true);

    let matches = Command::new("aperture")
        .subcommand(
            Command::new("users").subcommand(
                Command::new("get-user-by-id")
                    .arg(Arg::new("id").required(true))
                    .arg(Arg::new("body").long("body")),
            ),
        )
        .get_matches_from(vec![
            "aperture",
            "users",
            "get-user-by-id",
            "123",
            "--body",
            "{not-json}",
        ]);

    let err = matches_to_operation_call(&spec, &matches).expect_err("invalid JSON should fail");
    assert!(err.to_string().contains("Invalid JSON body"));
}

#[test]
fn extract_server_var_args_reads_repeated_flags() {
    let matches = Command::new("aperture")
        .subcommand(
            Command::new("users").subcommand(
                Command::new("get-user-by-id").arg(
                    Arg::new("server-var")
                        .long("server-var")
                        .global(true)
                        .action(ArgAction::Append),
                ),
            ),
        )
        .get_matches_from(vec![
            "aperture",
            "users",
            "get-user-by-id",
            "--server-var",
            "region=us",
            "--server-var",
            "env=prod",
        ]);

    let (_, users_matches) = matches.subcommand().expect("users subcommand should exist");
    let (_, operation_matches) = users_matches
        .subcommand()
        .expect("operation subcommand should exist");

    let args = extract_server_var_args(operation_matches);
    assert_eq!(args, vec!["region=us".to_string(), "env=prod".to_string()]);
}

#[test]
fn has_show_examples_flag_checks_deepest_subcommand() {
    let matches = build_matches(true);
    assert!(has_show_examples_flag(&matches));
}

#[test]
fn matches_to_operation_id_does_not_validate_body_json() {
    let spec = build_spec(vec![cached_parameter("id", "path", "string", true)], true);

    let matches = Command::new("aperture")
        .subcommand(
            Command::new("users").subcommand(
                Command::new("get-user-by-id")
                    .arg(Arg::new("id").required(true))
                    .arg(Arg::new("body").long("body"))
                    .arg(
                        Arg::new("show-examples")
                            .long("show-examples")
                            .action(ArgAction::SetTrue),
                    ),
            ),
        )
        .get_matches_from(vec![
            "aperture",
            "users",
            "get-user-by-id",
            "123",
            "--show-examples",
            "--body",
            "{not-json}",
        ]);

    let operation_id =
        matches_to_operation_id(&spec, &matches).expect("operation resolution should succeed");
    assert_eq!(operation_id, "getUserById");
}

#[test]
fn cli_to_execution_context_builds_retry_and_cache_settings() {
    let mut cli = base_cli();
    cli.dry_run = true;
    cli.idempotency_key = Some("idem-1".to_string());
    cli.cache = true;
    cli.cache_ttl = Some(42);
    cli.retry = Some(3);
    cli.retry_delay = Some("750ms".to_string());
    cli.retry_max_delay = Some("5s".to_string());
    cli.force_retry = true;

    let ctx = cli_to_execution_context(&cli, Some(GlobalConfig::default()))
        .expect("context construction should succeed");

    assert!(ctx.dry_run);
    assert_eq!(ctx.idempotency_key.as_deref(), Some("idem-1"));

    let cache = ctx.cache_config.expect("cache config should be present");
    assert!(cache.enabled);
    assert_eq!(cache.default_ttl.as_secs(), 42);

    let retry = ctx.retry_context.expect("retry should be enabled");
    assert_eq!(retry.max_attempts, 3);
    assert_eq!(retry.initial_delay_ms, 750);
    assert_eq!(retry.max_delay_ms, 5000);
    assert!(retry.force_retry);
    assert!(retry.has_idempotency_key);

    assert!(ctx.global_config.is_some());
    assert!(ctx.server_var_args.is_empty());
}

#[test]
fn cli_to_execution_context_disables_cache_when_no_cache_flag_is_set() {
    let mut cli = base_cli();
    cli.cache = true;
    cli.no_cache = true;

    let ctx = cli_to_execution_context(&cli, None).expect("context construction should succeed");
    assert!(ctx.cache_config.is_none());
}
