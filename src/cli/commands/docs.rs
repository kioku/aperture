//! Handlers for `aperture docs`, `aperture overview`, and `aperture commands`.

use crate::cache::models::{CachedCommand, CachedSpec};
use crate::cli::DiscoveryFormat;
use crate::config::manager::{get_config_dir, ConfigManager};
use crate::constants;
use crate::docs::{DocumentationGenerator, HelpFormatter};
use crate::engine::loader;
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::output::Output;
use crate::utils::to_kebab_case;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
struct ApiInfoJson {
    context: String,
    name: String,
    version: String,
    base_url: Option<String>,
    operation_count: usize,
}

#[derive(Debug, Serialize)]
struct MethodCountJson {
    method: String,
    count: usize,
}

#[derive(Debug, Serialize)]
struct CategoryCountJson {
    name: String,
    count: usize,
}

#[derive(Debug, Serialize)]
struct OperationSummaryJson {
    group: String,
    name: String,
    method: String,
    path: String,
    summary: Option<String>,
    description: Option<String>,
    deprecated: bool,
}

#[derive(Debug, Serialize)]
struct CommandGroupJson {
    name: String,
    operations: Vec<OperationSummaryJson>,
}

#[derive(Debug, Serialize)]
struct CommandListJson {
    api: ApiInfoJson,
    groups: Vec<CommandGroupJson>,
}

#[derive(Debug, Serialize)]
struct DocsInteractiveJson {
    mode: &'static str,
    apis: Vec<ApiInfoJson>,
}

#[derive(Debug, Serialize)]
struct DocsPathExampleJson {
    command: String,
    summary: String,
}

#[derive(Debug, Serialize)]
struct DocsReferenceJson {
    mode: &'static str,
    api: ApiInfoJson,
    categories: Vec<CategoryCountJson>,
    example_paths: Vec<DocsPathExampleJson>,
}

#[derive(Debug, Serialize)]
struct ParameterJson {
    name: String,
    cli_name: String,
    location: String,
    required: bool,
    description: Option<String>,
    schema: Option<String>,
    schema_type: Option<String>,
    format: Option<String>,
    default_value: Option<String>,
    enum_values: Vec<String>,
    example: Option<String>,
}

#[derive(Debug, Serialize)]
struct RequestBodyJson {
    content_type: String,
    schema: String,
    required: bool,
    description: Option<String>,
    example: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResponseJson {
    status_code: String,
    description: Option<String>,
    content_type: Option<String>,
    schema: Option<String>,
    example: Option<String>,
}

#[derive(Debug, Serialize)]
struct CommandExampleJson {
    description: String,
    command_line: String,
    explanation: Option<String>,
}

#[derive(Debug, Serialize)]
struct OperationDetailsJson {
    group: String,
    name: String,
    aliases: Vec<String>,
    method: String,
    path: String,
    summary: Option<String>,
    description: Option<String>,
    deprecated: bool,
    external_docs_url: Option<String>,
    usage: String,
    parameters: Vec<ParameterJson>,
    request_body: Option<RequestBodyJson>,
    responses: Vec<ResponseJson>,
    security_requirements: Vec<String>,
    examples: Vec<CommandExampleJson>,
}

#[derive(Debug, Serialize)]
struct DocsOperationJson {
    mode: &'static str,
    api: ApiInfoJson,
    operation: OperationDetailsJson,
}

#[derive(Debug, Serialize)]
struct OverviewStatisticsJson {
    total_operations: usize,
    methods: Vec<MethodCountJson>,
    categories: Vec<CategoryCountJson>,
}

#[derive(Debug, Serialize)]
struct OverviewQuickStartJson {
    commands: String,
    search: String,
    docs: String,
    describe_json: String,
}

#[derive(Debug, Serialize)]
struct SingleOverviewJson {
    api: ApiInfoJson,
    statistics: OverviewStatisticsJson,
    quick_start: OverviewQuickStartJson,
    sample_operations: Vec<OperationSummaryJson>,
}

#[derive(Debug, Serialize)]
struct ApiOverviewJson {
    context: String,
    name: String,
    version: String,
    base_url: Option<String>,
    operation_count: usize,
    methods: Vec<MethodCountJson>,
    quick_start: String,
}

#[derive(Debug, Serialize)]
struct AllOverviewJson {
    apis: Vec<ApiOverviewJson>,
}

pub fn list_commands(
    context: &str,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
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

    match format {
        DiscoveryFormat::Text => {
            let formatted_output = HelpFormatter::format_command_list(&spec);
            // ast-grep-ignore: no-println
            println!("{formatted_output}");
            output.tip(format!(
                "Next: 'aperture overview {context}' for high-level API orientation"
            ));
            output.tip(format!(
                "Next: 'aperture search <term> --api {context}' to find operations by intent"
            ));
            output.tip(format!(
                "Next: 'aperture docs {context} <tag> <operation>' for deep operation docs"
            ));
            output.tip(format!(
                "Execute: 'aperture api {context} <tag> <operation> ...'"
            ));
            Ok(())
        }
        DiscoveryFormat::Json => render_command_list_json(context, &spec),
    }
}

/// Execute help command with enhanced documentation
pub fn execute_help_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
    enhanced: bool,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
    match format {
        DiscoveryFormat::Text => {
            execute_help_command_text(manager, api_name, tag, operation, enhanced, output)
        }
        DiscoveryFormat::Json => execute_help_command_json(manager, api_name, tag, operation),
    }
}

fn execute_help_command_text(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
    enhanced: bool,
    output: &Output,
) -> Result<(), Error> {
    match (api_name, tag, operation) {
        (None, None, None) => render_interactive_menu(manager, output),
        (Some(api), None, None) => render_api_reference_index(manager, api, output),
        (Some(api), Some(tag), Some(op)) => {
            render_command_help(manager, api, tag, op, enhanced, output)
        }
        (Some(_), _, _) => {
            print_invalid_docs_usage();
        }
        _ => {
            print_invalid_help_arguments();
        }
    }
}

fn execute_help_command_json(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    tag: Option<&str>,
    operation: Option<&str>,
) -> Result<(), Error> {
    match (api_name, tag, operation) {
        (None, None, None) => render_interactive_menu_json(manager),
        (Some(api), None, None) => render_api_reference_index_json(manager, api),
        (Some(api), Some(tag), Some(op)) => render_command_help_json(manager, api, tag, op),
        (Some(_), _, _) => {
            print_invalid_docs_usage();
        }
        _ => {
            print_invalid_help_arguments();
        }
    }
}

fn render_interactive_menu(
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let doc_gen = DocumentationGenerator::new(specs);
    // ast-grep-ignore: no-println
    println!("{}", doc_gen.generate_interactive_menu());
    output.tip("Try 'aperture overview <api>' to orient to one API before drilling in");
    Ok(())
}

fn render_interactive_menu_json(manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let apis = specs
        .iter()
        .map(|(context, spec)| api_info(context, spec, spec.commands.len()))
        .collect::<Vec<_>>();
    print_json(&DocsInteractiveJson {
        mode: "interactive",
        apis,
    })
}

fn render_api_reference_index(
    manager: &ConfigManager<OsFileSystem>,
    api: &str,
    output: &Output,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let doc_gen = DocumentationGenerator::new(specs);
    let reference = doc_gen.generate_api_reference_index(api)?;
    // ast-grep-ignore: no-println
    println!("{reference}");
    output.tip(format!(
        "Execute operations with 'aperture api {api} <tag> <operation> ...'"
    ));
    output.tip(format!(
        "Machine workflow: 'aperture api {api} --describe-json'"
    ));
    Ok(())
}

fn render_api_reference_index_json(
    manager: &ConfigManager<OsFileSystem>,
    api: &str,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let spec = specs.get(api).ok_or_else(|| Error::spec_not_found(api))?;
    let visible_commands = spec
        .commands
        .iter()
        .filter(|command| !command.hidden)
        .collect::<Vec<_>>();

    let mut category_counts = BTreeMap::new();
    for command in &visible_commands {
        *category_counts
            .entry(DocumentationGenerator::effective_group(command))
            .or_insert(0usize) += 1;
    }

    let categories = category_counts
        .into_iter()
        .map(|(name, count)| CategoryCountJson { name, count })
        .collect::<Vec<_>>();

    let example_paths = visible_commands
        .iter()
        .take(3)
        .map(|command| {
            let group = DocumentationGenerator::effective_group(command);
            let operation = DocumentationGenerator::effective_operation(command);
            let summary = command
                .summary
                .as_deref()
                .or(command.description.as_deref())
                .unwrap_or("No description")
                .to_string();
            DocsPathExampleJson {
                command: format!("aperture docs {api} {group} {operation}"),
                summary,
            }
        })
        .collect::<Vec<_>>();

    print_json(&DocsReferenceJson {
        mode: "api-reference",
        api: api_info(api, spec, visible_commands.len()),
        categories,
        example_paths,
    })
}

fn render_command_help(
    manager: &ConfigManager<OsFileSystem>,
    api: &str,
    tag: &str,
    operation: &str,
    enhanced: bool,
    output: &Output,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let doc_gen = DocumentationGenerator::new(specs);
    let help = doc_gen.generate_command_help(api, tag, operation)?;
    if enhanced {
        // ast-grep-ignore: no-println
        println!("{help}");
    } else {
        // ast-grep-ignore: no-println
        println!("{}", help.lines().take(20).collect::<Vec<_>>().join("\n"));
        output.tip("Use --enhanced for full documentation with examples");
    }
    output.tip(format!(
        "Execute with 'aperture api {api} <tag> <operation> ...' after inspection"
    ));
    Ok(())
}

fn render_command_help_json(
    manager: &ConfigManager<OsFileSystem>,
    api: &str,
    tag: &str,
    operation: &str,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let spec = specs.get(api).ok_or_else(|| Error::spec_not_found(api))?;
    let command = find_docs_command(spec, api, tag, operation)?;

    print_json(&DocsOperationJson {
        mode: "operation",
        api: api_info(api, spec, spec.commands.len()),
        operation: build_operation_details_json(api, command),
    })
}

fn find_docs_command<'a>(
    spec: &'a CachedSpec,
    api: &str,
    tag: &str,
    operation: &str,
) -> Result<&'a CachedCommand, Error> {
    spec.commands
        .iter()
        .find(|command| DocumentationGenerator::matches_command_reference(command, tag, operation))
        .ok_or_else(|| {
            Error::spec_not_found(format!(
                "Operation '{tag} {operation}' not found in API '{api}'"
            ))
        })
}

fn build_operation_details_json(api: &str, command: &CachedCommand) -> OperationDetailsJson {
    let group = DocumentationGenerator::effective_group(command);
    let operation_name = DocumentationGenerator::effective_operation(command);
    let usage = DocumentationGenerator::canonical_usage(api, command);

    OperationDetailsJson {
        group,
        name: operation_name,
        aliases: command.aliases.clone(),
        method: command.method.clone(),
        path: command.path.clone(),
        summary: command.summary.clone(),
        description: command.description.clone(),
        deprecated: command.deprecated,
        external_docs_url: command.external_docs_url.clone(),
        usage,
        parameters: serialize_parameters(command),
        request_body: serialize_request_body(command),
        responses: serialize_responses(command),
        security_requirements: command.security_requirements.clone(),
        examples: serialize_examples(api, command),
    }
}

fn serialize_parameters(command: &CachedCommand) -> Vec<ParameterJson> {
    command
        .parameters
        .iter()
        .map(|param| ParameterJson {
            name: param.name.clone(),
            cli_name: to_kebab_case(&param.name),
            location: param.location.clone(),
            required: param.required,
            description: param.description.clone(),
            schema: param.schema.clone(),
            schema_type: param.schema_type.clone(),
            format: param.format.clone(),
            default_value: param.default_value.clone(),
            enum_values: param.enum_values.clone(),
            example: param.example.clone(),
        })
        .collect()
}

fn serialize_request_body(command: &CachedCommand) -> Option<RequestBodyJson> {
    command.request_body.as_ref().map(|body| RequestBodyJson {
        content_type: body.content_type.clone(),
        schema: body.schema.clone(),
        required: body.required,
        description: body.description.clone(),
        example: body.example.clone(),
    })
}

fn serialize_responses(command: &CachedCommand) -> Vec<ResponseJson> {
    command
        .responses
        .iter()
        .map(|response| ResponseJson {
            status_code: response.status_code.clone(),
            description: response.description.clone(),
            content_type: response.content_type.clone(),
            schema: response.schema.clone(),
            example: response.example.clone(),
        })
        .collect()
}

fn serialize_examples(api: &str, command: &CachedCommand) -> Vec<CommandExampleJson> {
    DocumentationGenerator::canonical_examples(api, command)
        .into_iter()
        .map(|example| CommandExampleJson {
            description: example.description,
            command_line: example.command_line,
            explanation: example.explanation,
        })
        .collect()
}

fn print_invalid_docs_usage() -> ! {
    // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
    // ast-grep-ignore: no-println
    eprintln!("Invalid docs command. Usage:");
    // ast-grep-ignore: no-println
    eprintln!("  aperture docs                        # Interactive menu");
    // ast-grep-ignore: no-println
    eprintln!("  aperture docs <api>                  # API reference index");
    // ast-grep-ignore: no-println
    eprintln!("  aperture docs <api> <tag> <operation> # Command help");
    std::process::exit(1);
}

fn print_invalid_help_arguments() -> ! {
    // Must appear regardless of APERTURE_LOG; tracing may suppress at low levels.
    // ast-grep-ignore: no-println
    eprintln!("Invalid help command arguments");
    std::process::exit(1);
}

/// Execute overview command
#[allow(clippy::too_many_lines)]
pub fn execute_overview_command(
    manager: &ConfigManager<OsFileSystem>,
    api_name: Option<&str>,
    all: bool,
    format: &DiscoveryFormat,
    output: &Output,
) -> Result<(), Error> {
    if !all {
        let Some(api) = api_name else {
            print_overview_usage();
        };
        return match format {
            DiscoveryFormat::Text => render_single_api_overview(manager, api, output),
            DiscoveryFormat::Json => render_single_api_overview_json(manager, api),
        };
    }

    match format {
        DiscoveryFormat::Text => render_all_api_overviews(manager, output),
        DiscoveryFormat::Json => render_all_api_overviews_json(manager),
    }
}

fn render_single_api_overview(
    manager: &ConfigManager<OsFileSystem>,
    api: &str,
    output: &Output,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let doc_gen = DocumentationGenerator::new(specs);
    let overview = doc_gen.generate_api_overview(api)?;
    // ast-grep-ignore: no-println
    println!("{overview}");
    output.tip(format!(
        "Next: 'aperture search <term> --api {api}' to find specific operations"
    ));
    output.tip(format!(
        "Next: 'aperture commands {api}' for a terse command tree"
    ));
    output.tip(format!(
        "Next: 'aperture docs {api} <tag> <operation>' for deep operation reference"
    ));
    output.tip(format!(
        "Machine workflow: 'aperture api {api} --describe-json'"
    ));
    Ok(())
}

fn render_single_api_overview_json(
    manager: &ConfigManager<OsFileSystem>,
    api: &str,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let spec = specs.get(api).ok_or_else(|| Error::spec_not_found(api))?;
    let visible_commands = spec
        .commands
        .iter()
        .filter(|command| !command.hidden)
        .collect::<Vec<_>>();

    let method_counts = collect_method_counts(visible_commands.iter().copied());
    let category_counts = collect_category_counts(&visible_commands);
    let sample_operations = visible_commands
        .iter()
        .take(3)
        .map(|command| operation_summary(command))
        .collect::<Vec<_>>();

    print_json(&SingleOverviewJson {
        api: api_info(api, spec, visible_commands.len()),
        statistics: OverviewStatisticsJson {
            total_operations: visible_commands.len(),
            methods: method_counts,
            categories: category_counts,
        },
        quick_start: OverviewQuickStartJson {
            commands: format!("aperture commands {api}"),
            search: format!("aperture search \"keyword\" --api {api}"),
            docs: format!("aperture docs {api} <tag> <operation>"),
            describe_json: format!("aperture api {api} --describe-json"),
        },
        sample_operations,
    })
}

fn render_all_api_overviews(
    manager: &ConfigManager<OsFileSystem>,
    output: &Output,
) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    if specs.is_empty() {
        output.info("No API specifications configured.");
        output.info("Use 'aperture config api add <name> <spec-file>' to get started.");
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
        let method_summary = summarize_methods(&spec.commands);
        // ast-grep-ignore: no-println
        println!("   Methods: {}", method_summary.join(", "));
        // ast-grep-ignore: no-println
        println!("   Quick start: aperture commands {api_name}");
    }
    // ast-grep-ignore: no-println
    println!("\n{}", "=".repeat(60));
    output.tip("Use 'aperture overview <api>' to orient to a specific API");
    output.tip("Then use 'aperture search <term> --api <api>' to find operations by intent");
    output.tip("Use 'aperture commands <api>' for a terse command tree");
    output.tip("Use 'aperture docs <api> <tag> <operation>' for deep operation reference");
    Ok(())
}

fn render_all_api_overviews_json(manager: &ConfigManager<OsFileSystem>) -> Result<(), Error> {
    let specs = load_all_specs(manager)?;
    let apis = specs
        .iter()
        .map(|(context, spec)| ApiOverviewJson {
            context: context.clone(),
            name: spec.name.clone(),
            version: spec.version.clone(),
            base_url: spec.base_url.clone(),
            operation_count: spec.commands.len(),
            methods: collect_method_counts(spec.commands.iter()),
            quick_start: format!("aperture commands {context}"),
        })
        .collect::<Vec<_>>();

    print_json(&AllOverviewJson { apis })
}

fn render_command_list_json(context: &str, spec: &CachedSpec) -> Result<(), Error> {
    let visible_commands = spec
        .commands
        .iter()
        .filter(|command| !command.hidden)
        .collect::<Vec<_>>();

    let mut groups = BTreeMap::<String, Vec<OperationSummaryJson>>::new();
    for command in &visible_commands {
        groups
            .entry(DocumentationGenerator::effective_group(command))
            .or_default()
            .push(operation_summary(command));
    }

    let groups = groups
        .into_iter()
        .map(|(name, operations)| CommandGroupJson { name, operations })
        .collect::<Vec<_>>();

    print_json(&CommandListJson {
        api: api_info(context, spec, visible_commands.len()),
        groups,
    })
}

fn collect_method_counts<'a, I>(commands: I) -> Vec<MethodCountJson>
where
    I: Iterator<Item = &'a CachedCommand>,
{
    let mut method_counts = BTreeMap::new();
    for command in commands {
        *method_counts.entry(command.method.clone()).or_insert(0) += 1;
    }
    method_counts
        .into_iter()
        .map(|(method, count)| MethodCountJson { method, count })
        .collect()
}

fn collect_category_counts(commands: &[&CachedCommand]) -> Vec<CategoryCountJson> {
    let mut category_counts = BTreeMap::new();
    for command in commands {
        *category_counts
            .entry(DocumentationGenerator::effective_group(command))
            .or_insert(0usize) += 1;
    }
    category_counts
        .into_iter()
        .map(|(name, count)| CategoryCountJson { name, count })
        .collect()
}

fn api_info(context: &str, spec: &CachedSpec, operation_count: usize) -> ApiInfoJson {
    ApiInfoJson {
        context: context.to_string(),
        name: spec.name.clone(),
        version: spec.version.clone(),
        base_url: spec.base_url.clone(),
        operation_count,
    }
}

fn operation_summary(command: &CachedCommand) -> OperationSummaryJson {
    OperationSummaryJson {
        group: DocumentationGenerator::effective_group(command),
        name: DocumentationGenerator::effective_operation(command),
        method: command.method.clone(),
        path: command.path.clone(),
        summary: command.summary.clone(),
        description: command.description.clone(),
        deprecated: command.deprecated,
    }
}

fn print_json<T: Serialize>(payload: &T) -> Result<(), Error> {
    // ast-grep-ignore: no-println
    println!("{}", serde_json::to_string_pretty(payload)?);
    Ok(())
}

fn summarize_methods(spec_commands: &[crate::cache::models::CachedCommand]) -> Vec<String> {
    collect_method_counts(spec_commands.iter())
        .into_iter()
        .map(|entry| format!("{}: {}", entry.method, entry.count))
        .collect()
}

fn print_overview_usage() -> ! {
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
