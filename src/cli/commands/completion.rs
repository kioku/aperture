use crate::cache::models::{CachedCommand, CachedSpec};
use crate::cli::CompletionShell;
use crate::config::manager::{get_config_dir, ConfigManager};
use crate::constants;
use crate::engine::loader;
use crate::error::Error;
use crate::fs::OsFileSystem;
use crate::utils::to_kebab_case;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

const TOP_LEVEL_COMMANDS: &[&str] = &[
    "completion",
    "config",
    "commands",
    "list-commands",
    "api",
    "search",
    "run",
    "exec",
    "docs",
    "overview",
];

const GLOBAL_FLAGS: &[&str] = &["--help", "--json-errors", "--quiet", "-q", "-v"];

const CONFIG_DOMAINS: &[&str] = &["api", "url", "secret", "cache", "setting", "mapping"];
const CONFIG_API_COMMANDS: &[&str] = &["add", "list", "remove", "edit", "reinit"];
const CONFIG_URL_COMMANDS: &[&str] = &["set", "get", "list"];
const CONFIG_SECRET_COMMANDS: &[&str] = &["set", "list", "remove", "clear"];
const CONFIG_CACHE_COMMANDS: &[&str] = &["clear", "stats"];
const CONFIG_SETTING_COMMANDS: &[&str] = &["set", "get", "list"];
const CONFIG_MAPPING_COMMANDS: &[&str] = &["set", "list", "remove"];

const SHELL_NAMES: &[&str] = &["bash", "zsh", "fish", "nu", "powershell"];

const API_EXECUTION_FLAGS: &[&str] = &[
    "--help",
    "--describe-json",
    "--dry-run",
    "--idempotency-key",
    "--format",
    "--jq",
    "--batch-file",
    "--batch-concurrency",
    "--batch-rate-limit",
    "--cache",
    "--no-cache",
    "--cache-ttl",
    "--positional-args",
    "--auto-paginate",
    "--retry",
    "--retry-delay",
    "--retry-max-delay",
    "--force-retry",
];

const API_PREFIX_FLAGS_WITH_VALUES: &[&str] = &[
    "--idempotency-key",
    "--format",
    "--jq",
    "--batch-file",
    "--batch-concurrency",
    "--batch-rate-limit",
    "--cache-ttl",
    "--retry",
    "--retry-delay",
    "--retry-max-delay",
];

const API_DYNAMIC_GLOBAL_FLAGS: &[&str] = &["--help", "--jq", "--format", "--server-var"];

#[derive(Default)]
struct CompletionCatalog {
    contexts: Vec<String>,
    specs: HashMap<String, CachedSpec>,
}

struct CompletionInput {
    before_cursor: Vec<String>,
    current: String,
}

#[derive(Default)]
struct ApiCompletionState {
    context: Option<String>,
    dynamic_path: Vec<String>,
}

pub fn execute_completion_script_command(shell: &CompletionShell) -> Result<(), Error> {
    let script = match shell {
        CompletionShell::Bash => bash_completion_script(),
        CompletionShell::Zsh => zsh_completion_script(),
        CompletionShell::Fish => fish_completion_script(),
        CompletionShell::Nu => nu_completion_script(),
        CompletionShell::PowerShell => powershell_completion_script(),
    };

    // ast-grep-ignore: no-println
    println!("{script}");
    Ok(())
}

pub fn execute_completion_runtime_command(
    _shell: &CompletionShell,
    cword: usize,
    words: &[String],
) -> Result<(), Error> {
    let catalog = load_completion_catalog().unwrap_or_default();
    let input = normalize_completion_input(words, cword);
    let suggestions = complete_words(&input, &catalog);

    for suggestion in suggestions {
        // ast-grep-ignore: no-println
        println!("{suggestion}");
    }

    Ok(())
}

fn normalize_completion_input(words: &[String], cword: usize) -> CompletionInput {
    let (trimmed_words, adjusted_cword) = if words.first().is_some_and(|word| is_binary_name(word))
    {
        (&words[1..], cword.saturating_sub(1))
    } else {
        (words, cword)
    };

    let safe_cursor = adjusted_cword.min(trimmed_words.len());
    let current = trimmed_words.get(safe_cursor).cloned().unwrap_or_default();
    let before_cursor = trimmed_words[..safe_cursor].to_vec();

    CompletionInput {
        before_cursor,
        current,
    }
}

fn is_binary_name(word: &str) -> bool {
    word == "aperture" || word.ends_with("/aperture")
}

fn complete_words(input: &CompletionInput, catalog: &CompletionCatalog) -> Vec<String> {
    let before_cursor = strip_leading_global_flags(&input.before_cursor);

    let Some(command) = before_cursor.first().map(String::as_str) else {
        return filter_candidates(top_level_candidates(), &input.current);
    };

    let args_after_command = strip_leading_global_flags(&before_cursor[1..]);

    if let Some(primary) = complete_primary_command(command, args_after_command, input, catalog) {
        return primary;
    }

    complete_secondary_command(command, args_after_command, input, catalog)
}

fn strip_leading_global_flags(tokens: &[String]) -> &[String] {
    let mut index = 0;

    while tokens
        .get(index)
        .is_some_and(|token| is_global_flag_token(token))
    {
        index += 1;
    }

    &tokens[index..]
}

fn is_global_flag_token(token: &str) -> bool {
    GLOBAL_FLAGS.iter().any(|flag| flag == &token)
        || (token.starts_with('-') && token.chars().skip(1).all(|ch| ch == 'v'))
}

fn complete_primary_command(
    command: &str,
    args_after_command: &[String],
    input: &CompletionInput,
    catalog: &CompletionCatalog,
) -> Option<Vec<String>> {
    match command {
        "completion" => Some(filter_candidates(
            SHELL_NAMES.iter().map(ToString::to_string),
            &input.current,
        )),
        "config" => Some(complete_config(args_after_command, &input.current, catalog)),
        "commands" | "list-commands" => Some(complete_single_context_argument(
            args_after_command,
            &input.current,
            &catalog.contexts,
        )),
        "api" => Some(complete_api(args_after_command, &input.current, catalog)),
        _ => None,
    }
}

fn complete_secondary_command(
    command: &str,
    args_after_command: &[String],
    input: &CompletionInput,
    catalog: &CompletionCatalog,
) -> Vec<String> {
    match command {
        "docs" => complete_docs(args_after_command, &input.current, catalog),
        "overview" => complete_overview(args_after_command, &input.current, &catalog.contexts),
        "search" => complete_search(args_after_command, &input.current, &catalog.contexts),
        "run" | "exec" => complete_run(args_after_command, &input.current, &catalog.contexts),
        _ => Vec::new(),
    }
}

fn top_level_candidates() -> impl Iterator<Item = String> {
    TOP_LEVEL_COMMANDS
        .iter()
        .chain(GLOBAL_FLAGS.iter())
        .map(ToString::to_string)
}

fn complete_single_context_argument(
    args: &[String],
    current: &str,
    contexts: &[String],
) -> Vec<String> {
    if args.is_empty() {
        return filter_candidates(contexts.iter().cloned(), current);
    }

    Vec::new()
}

fn complete_overview(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    if args.is_empty() {
        return filter_candidates(
            contexts
                .iter()
                .cloned()
                .chain(std::iter::once("--all".to_string())),
            current,
        );
    }

    Vec::new()
}

fn complete_search(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    if args.last().is_some_and(|arg| arg == "--api") {
        return filter_candidates(contexts.iter().cloned(), current);
    }

    Vec::new()
}

fn complete_run(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    if args.last().is_some_and(|arg| arg == "--api") {
        return filter_candidates(contexts.iter().cloned(), current);
    }

    Vec::new()
}

fn complete_docs(args: &[String], current: &str, catalog: &CompletionCatalog) -> Vec<String> {
    if args.is_empty() {
        return filter_candidates(catalog.contexts.iter().cloned(), current);
    }

    let Some(context) = args.first() else {
        return Vec::new();
    };
    let Some(spec) = catalog.specs.get(context) else {
        return Vec::new();
    };

    if args.len() == 1 {
        return filter_candidates(group_names(spec), current);
    }
    if args.len() == 2 {
        return filter_candidates(operation_names_for_group(spec, &args[1]), current);
    }

    Vec::new()
}

fn complete_config(args: &[String], current: &str, catalog: &CompletionCatalog) -> Vec<String> {
    let Some(domain) = args.first().map(String::as_str) else {
        return filter_candidates(CONFIG_DOMAINS.iter().map(ToString::to_string), current);
    };

    let rest = &args[1..];

    match domain {
        "api" => complete_config_api(rest, current, &catalog.contexts),
        "url" => complete_config_url(rest, current, &catalog.contexts),
        "secret" => complete_config_secret(rest, current, &catalog.contexts),
        "cache" => complete_config_cache(rest, current, &catalog.contexts),
        "setting" => complete_config_setting(rest, current),
        "mapping" => complete_config_mapping(rest, current, &catalog.contexts),
        _ => Vec::new(),
    }
}

fn complete_config_api(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return filter_candidates(CONFIG_API_COMMANDS.iter().map(ToString::to_string), current);
    };

    match command {
        "remove" | "edit" => complete_when_only_subcommand_token(args, current, contexts),
        "reinit" => {
            if args.len() == 1 {
                return filter_candidates(
                    contexts
                        .iter()
                        .cloned()
                        .chain(std::iter::once("--all".to_string())),
                    current,
                );
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn complete_config_url(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return filter_candidates(CONFIG_URL_COMMANDS.iter().map(ToString::to_string), current);
    };

    match command {
        "set" | "get" => complete_when_only_subcommand_token(args, current, contexts),
        _ => Vec::new(),
    }
}

fn complete_config_secret(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return filter_candidates(
            CONFIG_SECRET_COMMANDS.iter().map(ToString::to_string),
            current,
        );
    };

    match command {
        "set" | "list" | "remove" | "clear" => {
            complete_when_only_subcommand_token(args, current, contexts)
        }
        _ => Vec::new(),
    }
}

fn complete_config_cache(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return filter_candidates(
            CONFIG_CACHE_COMMANDS.iter().map(ToString::to_string),
            current,
        );
    };

    match command {
        "clear" => {
            if args.len() == 1 {
                return filter_candidates(
                    contexts
                        .iter()
                        .cloned()
                        .chain(std::iter::once("--all".to_string())),
                    current,
                );
            }
            Vec::new()
        }
        "stats" => complete_when_only_subcommand_token(args, current, contexts),
        _ => Vec::new(),
    }
}

fn complete_config_setting(args: &[String], current: &str) -> Vec<String> {
    if args.is_empty() {
        return filter_candidates(
            CONFIG_SETTING_COMMANDS.iter().map(ToString::to_string),
            current,
        );
    }

    Vec::new()
}

fn complete_config_mapping(args: &[String], current: &str, contexts: &[String]) -> Vec<String> {
    let Some(command) = args.first().map(String::as_str) else {
        return filter_candidates(
            CONFIG_MAPPING_COMMANDS.iter().map(ToString::to_string),
            current,
        );
    };

    match command {
        "set" | "list" | "remove" => complete_when_only_subcommand_token(args, current, contexts),
        _ => Vec::new(),
    }
}

fn complete_when_only_subcommand_token(
    args: &[String],
    current: &str,
    contexts: &[String],
) -> Vec<String> {
    if args.len() == 1 {
        return filter_candidates(contexts.iter().cloned(), current);
    }

    Vec::new()
}

fn complete_api(args: &[String], current: &str, catalog: &CompletionCatalog) -> Vec<String> {
    let state = parse_api_completion_state(args);

    let Some(context) = state.context.as_deref() else {
        return filter_candidates(
            catalog
                .contexts
                .iter()
                .cloned()
                .chain(API_EXECUTION_FLAGS.iter().map(ToString::to_string)),
            current,
        );
    };

    let Some(spec) = catalog.specs.get(context) else {
        return Vec::new();
    };

    if state.dynamic_path.is_empty() {
        return filter_candidates(
            group_names(spec)
                .into_iter()
                .chain(API_EXECUTION_FLAGS.iter().map(ToString::to_string)),
            current,
        );
    }

    let Some(group) = state.dynamic_path.first() else {
        return Vec::new();
    };

    if state.dynamic_path.len() == 1 {
        return filter_candidates(operation_names_for_group(spec, group), current);
    }

    let Some(operation) = state.dynamic_path.get(1) else {
        return Vec::new();
    };

    filter_candidates(operation_flags(spec, group, operation), current)
}

fn parse_api_completion_state(args: &[String]) -> ApiCompletionState {
    let mut state = ApiCompletionState::default();
    let mut index = 0;

    while index < args.len() {
        let token = &args[index];

        if state.context.is_none() && token.starts_with('-') {
            index += api_execution_flag_step(token);
            continue;
        }

        if state.context.is_none() {
            state.context = Some(token.clone());
            index += 1;
            continue;
        }

        if state.dynamic_path.is_empty() && token.starts_with('-') {
            index += api_execution_flag_step(token);
            continue;
        }

        state.dynamic_path.push(token.clone());
        index += 1;
    }

    state
}

fn api_execution_flag_step(token: &str) -> usize {
    if token.contains('=') {
        return 1;
    }

    if API_PREFIX_FLAGS_WITH_VALUES
        .iter()
        .any(|flag| flag == &token)
    {
        return 2;
    }

    1
}

fn group_names(spec: &CachedSpec) -> Vec<String> {
    spec.commands
        .iter()
        .filter(|command| !command.hidden)
        .map(effective_group_name)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn operation_names_for_group(spec: &CachedSpec, group: &str) -> Vec<String> {
    spec.commands
        .iter()
        .filter(|command| !command.hidden && effective_group_name(command) == group)
        .flat_map(|command| {
            std::iter::once(effective_operation_name(command))
                .chain(command.aliases.iter().map(|alias| to_kebab_case(alias)))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn operation_flags(spec: &CachedSpec, group: &str, operation: &str) -> Vec<String> {
    let Some(command) = find_command_for_operation(spec, group, operation) else {
        return Vec::new();
    };

    let mut flags = command
        .parameters
        .iter()
        .map(|parameter| format!("--{}", to_kebab_case(&parameter.name)))
        .collect::<BTreeSet<_>>();

    if command.request_body.is_some() {
        flags.insert("--body".to_string());
        flags.insert("--body-file".to_string());
    }

    flags.insert("--header".to_string());
    flags.insert("-H".to_string());
    flags.insert("--show-examples".to_string());

    for flag in API_DYNAMIC_GLOBAL_FLAGS {
        flags.insert((*flag).to_string());
    }

    flags.into_iter().collect()
}

fn find_command_for_operation<'a>(
    spec: &'a CachedSpec,
    group: &str,
    operation: &str,
) -> Option<&'a CachedCommand> {
    spec.commands.iter().find(|command| {
        if command.hidden || effective_group_name(command) != group {
            return false;
        }

        if effective_operation_name(command) == operation {
            return true;
        }

        command
            .aliases
            .iter()
            .map(|alias| to_kebab_case(alias))
            .any(|alias| alias == operation)
    })
}

fn effective_group_name(command: &CachedCommand) -> String {
    command.display_group.as_ref().map_or_else(
        || {
            if command.name.is_empty() {
                constants::DEFAULT_GROUP.to_string()
            } else {
                to_kebab_case(&command.name)
            }
        },
        |display_group| to_kebab_case(display_group),
    )
}

fn effective_operation_name(command: &CachedCommand) -> String {
    command.display_name.as_ref().map_or_else(
        || {
            if command.operation_id.is_empty() {
                command.method.to_lowercase()
            } else {
                to_kebab_case(&command.operation_id)
            }
        },
        |display_name| to_kebab_case(display_name),
    )
}

fn filter_candidates<I>(candidates: I, current: &str) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    candidates
        .into_iter()
        .filter(|candidate| current.is_empty() || candidate.starts_with(current))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn load_completion_catalog() -> Result<CompletionCatalog, Error> {
    let manager = build_manager()?;

    let mut contexts = manager.list_specs()?;
    contexts.sort_unstable();

    let cache_dir = manager.config_dir().join(constants::DIR_CACHE);
    let specs = contexts
        .iter()
        .filter_map(|context| {
            loader::load_cached_spec(&cache_dir, context)
                .ok()
                .map(|spec| (context.clone(), spec))
        })
        .collect();

    Ok(CompletionCatalog { contexts, specs })
}

fn build_manager() -> Result<ConfigManager<OsFileSystem>, Error> {
    if let Ok(config_dir) = std::env::var(constants::ENV_APERTURE_CONFIG_DIR) {
        return Ok(ConfigManager::with_fs(
            OsFileSystem,
            PathBuf::from(config_dir),
        ));
    }

    let config_dir = get_config_dir()?;
    Ok(ConfigManager::with_fs(OsFileSystem, config_dir))
}

fn bash_completion_script() -> String {
    r#"_aperture_completion() {
    local IFS=$'\n'
    local output
    output="$(aperture __complete bash "${COMP_CWORD}" "${COMP_WORDS[@]}" 2>/dev/null)"

    COMPREPLY=()
    for line in $output; do
        COMPREPLY+=("$line")
    done
}

complete -F _aperture_completion aperture
"#
    .to_string()
}

fn zsh_completion_script() -> String {
    r#"#compdef aperture

_aperture_completion() {
  local -a candidates
  candidates=("${(@f)$(aperture __complete zsh $((CURRENT - 1)) "${words[@]}" 2>/dev/null)}")
  _describe 'aperture commands' candidates
}

compdef _aperture_completion aperture
"#
    .to_string()
}

fn fish_completion_script() -> String {
    r#"function __aperture_complete
    set -l words (commandline -opc)
    set -l cword (math (count $words) - 1)

    if test $cword -lt 0
        set cword 0
    end

    aperture __complete fish $cword $words 2>/dev/null
end

complete -c aperture -f -a "(__aperture_complete)"
"#
    .to_string()
}

fn nu_completion_script() -> String {
    r#"let previous_aperture_completer = (try {
    $env.config.completions.external.completer
} catch {
    null
})

let aperture_completer = {|spans|
    if ($spans | is-empty) {
        return []
    }

    if $spans.0 == "aperture" {
        ^aperture __complete nu (($spans | length) - 1) ...$spans
        | lines
        | where {|line| $line != "" }
        | each {|line| { value: $line, description: "" } }
    } else if $previous_aperture_completer != null {
        do $previous_aperture_completer $spans
    } else {
        []
    }
}

$env.config = ($env.config
    | upsert completions.external.enable true
    | upsert completions.external.completer $aperture_completer
)
"#
    .to_string()
}

fn powershell_completion_script() -> String {
    r"Register-ArgumentCompleter -Native -CommandName aperture -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $words = @($commandAst.CommandElements | ForEach-Object { $_.Extent.Text })
    if ($words.Count -eq 0) {
        return
    }

    $cword = $words.Count - 1
    $results = aperture __complete powershell $cword @words 2>$null

    foreach ($result in $results) {
        [System.Management.Automation.CompletionResult]::new($result, $result, 'ParameterValue', $result)
    }
}
"
    .to_string()
}
