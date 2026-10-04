//! Command search functionality for discovering API operations
//!
//! This module provides search capabilities to help users find relevant
//! API operations across registered specifications using fuzzy matching
//! and keyword search.

use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec};
use crate::constants;
use crate::discovery_style::DiscoveryStyle;
use crate::error::Error;
use crate::utils::to_kebab_case;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use regex::Regex;
use std::collections::BTreeMap;

/// Search result for a command
#[derive(Debug, Clone)]
pub struct CommandSearchResult {
    /// The API context name
    pub api_context: String,
    /// The matching command
    pub command: CachedCommand,
    /// The command path (e.g., "users get-user")
    pub command_path: String,
    /// The relevance score (higher is better)
    pub score: i64,
    /// Match highlights
    pub highlights: Vec<String>,
}

/// Internal scoring result for a command match
#[derive(Debug, Default)]
struct ScoringResult {
    /// The relevance score (higher is better)
    score: i64,
    /// Match highlights
    highlights: Vec<String>,
}

/// Command searcher for finding operations across APIs
pub struct CommandSearcher {
    /// Fuzzy matcher for similarity scoring
    matcher: SkimMatcherV2,
}

impl CommandSearcher {
    /// Create a new command searcher
    #[must_use]
    pub fn new() -> Self {
        Self {
            matcher: SkimMatcherV2::default().ignore_case(),
        }
    }

    /// Search for commands across specifications
    ///
    /// # Arguments
    /// * `specs` - Map of API context names to cached specifications
    /// * `query` - The search query (keywords or regex)
    /// * `api_filter` - Optional API context to limit search
    ///
    /// # Returns
    /// A vector of search results sorted by relevance
    ///
    /// # Errors
    /// Returns an error if regex compilation fails
    pub fn search(
        &self,
        specs: &BTreeMap<String, CachedSpec>,
        query: &str,
        api_filter: Option<&str>,
    ) -> Result<Vec<CommandSearchResult>, Error> {
        let mut results = Vec::new();
        if query.trim().is_empty() {
            return Ok(results);
        }

        // Regex is opt-in; ordinary words always use keyword/fuzzy matching.
        let regex_pattern = compile_search_regex(query)?;

        for (api_name, spec) in specs {
            // Apply API filter if specified - early continue if filter doesn't match
            if api_filter.is_some_and(|filter| api_name != filter) {
                continue;
            }

            for command in &spec.commands {
                // Score this command against the query
                let score_result = self.score_command(command, query, regex_pattern.as_ref());

                // Only include results with positive scores
                if score_result.score > 0 {
                    let command_path = effective_command_path(command);

                    results.push(CommandSearchResult {
                        api_context: api_name.clone(),
                        command: command.clone(),
                        command_path,
                        score: score_result.score,
                        highlights: score_result.highlights,
                    });
                }
            }
        }

        // Sort by score (highest first)
        results.sort_by(compare_search_results);

        Ok(results)
    }

    /// Score a single command against a query using regex or fuzzy matching
    fn score_command(
        &self,
        command: &CachedCommand,
        query: &str,
        regex_pattern: Option<&Regex>,
    ) -> ScoringResult {
        let operation_id_kebab = to_kebab_case(&command.operation_id);
        let summary = command.summary.as_deref().unwrap_or("");
        let description = command.description.as_deref().unwrap_or("");

        // Build searchable text from command attributes, including display overrides and aliases
        let display_name = command
            .display_name
            .as_deref()
            .map(to_kebab_case)
            .unwrap_or_default();
        let display_group = command
            .display_group
            .as_deref()
            .map(to_kebab_case)
            .unwrap_or_default();
        let aliases_text = command.aliases.join(" ");

        let search_text = format!(
            "{operation_id_kebab} {} {} {} {summary} {description} {display_name} {display_group} {aliases_text}",
            command.operation_id, command.method, command.path
        );

        // Score based on different matching strategies
        regex_pattern.map_or_else(
            || self.score_with_fuzzy_match(command, query, &operation_id_kebab),
            |regex| Self::score_with_regex(regex, &search_text),
        )
    }

    /// Score a command using regex matching
    fn score_with_regex(regex: &Regex, search_text: &str) -> ScoringResult {
        // Regex mode - only score if it matches
        if !regex.is_match(search_text) {
            return ScoringResult::default();
        }

        // Dynamic scoring based on match quality for regex
        let base_score = 90;
        let query_len = regex.as_str().len();
        #[allow(clippy::cast_possible_wrap)]
        let match_specificity_bonus = query_len.min(10) as i64;
        let total_score = base_score + match_specificity_bonus;

        ScoringResult {
            score: total_score,
            highlights: vec![format!("Regex match: {}", regex.as_str())],
        }
    }

    /// Score a command using fuzzy matching and substring bonuses
    fn score_with_fuzzy_match(
        &self,
        command: &CachedCommand,
        query: &str,
        operation_id_kebab: &str,
    ) -> ScoringResult {
        if query.split_whitespace().count() > 1 {
            return score_intent(command, query, operation_id_kebab);
        }
        let mut highlights = Vec::new();
        let normalized_query = normalize_keyword(query);
        let normalized_name = normalize_keyword(operation_id_kebab);
        let mut total_score = name_similarity_score(&normalized_name, &normalized_query);

        // Fuzzy matching stays within operation names to avoid cross-field accidents.
        if let Some(score) = self.matcher.fuzzy_match(operation_id_kebab, query) {
            total_score += score.clamp(0, 100);
        }

        // Bonus score for exact substring matches in various fields
        let query_lower = query.to_lowercase();

        Self::add_field_bonus(
            &query_lower,
            operation_id_kebab,
            "Operation",
            50,
            &mut total_score,
            &mut highlights,
        );
        Self::add_field_bonus(
            &query_lower,
            &command.operation_id,
            "Operation",
            50,
            &mut total_score,
            &mut highlights,
        );
        Self::add_field_bonus(
            &query_lower,
            &command.method,
            "Method",
            30,
            &mut total_score,
            &mut highlights,
        );
        Self::add_field_bonus(
            &query_lower,
            &command.path,
            "Path",
            20,
            &mut total_score,
            &mut highlights,
        );

        // Summary requires special handling for Option type
        if let Some(summary) = &command.summary {
            Self::add_field_bonus(
                &query_lower,
                summary,
                "Summary",
                15,
                &mut total_score,
                &mut highlights,
            );
        }

        if let Some(description) = &command.description {
            Self::add_field_bonus(
                &query_lower,
                description,
                "Description",
                10,
                &mut total_score,
                &mut highlights,
            );
        }

        // Bonus for display name matches (custom command names)
        if let Some(display_name) = &command.display_name {
            Self::add_field_bonus(
                &query_lower,
                display_name,
                "Display name",
                50,
                &mut total_score,
                &mut highlights,
            );
        }

        // Alias count must not let incidental matches outrank exact operation names.
        if let Some(alias) = command
            .aliases
            .iter()
            .find(|alias| alias.to_lowercase().contains(&query_lower))
        {
            Self::add_field_bonus(
                &query_lower,
                alias,
                "Alias",
                45,
                &mut total_score,
                &mut highlights,
            );
        }

        ScoringResult {
            score: total_score,
            highlights,
        }
    }

    /// Add bonus score if a field value contains the query string
    fn add_field_bonus(
        query_lower: &str,
        field_value: &str,
        field_label: &str,
        score: i64,
        total_score: &mut i64,
        highlights: &mut Vec<String>,
    ) {
        if field_value.to_lowercase().contains(query_lower) {
            *total_score += score;
            highlights.push(format!("{field_label}: {field_value}"));
        }
    }

    /// Find similar commands to a given input
    ///
    /// This is used for "did you mean?" suggestions on errors
    pub fn find_similar_commands(
        &self,
        spec: &CachedSpec,
        input: &str,
        max_results: usize,
    ) -> Vec<(String, i64)> {
        let mut suggestions = Vec::new();

        for command in &spec.commands {
            let full_command = effective_command_path(command);

            Self::push_fuzzy_match(
                &mut suggestions,
                &full_command,
                self.matcher.fuzzy_match(&full_command, input),
                0,
            );

            let effective_name = command
                .display_name
                .as_deref()
                .map_or_else(|| to_kebab_case(&command.operation_id), to_kebab_case);
            Self::push_fuzzy_match(
                &mut suggestions,
                &full_command,
                self.matcher.fuzzy_match(&effective_name, input),
                10,
            );

            for alias in &command.aliases {
                let alias_kebab = to_kebab_case(alias);
                Self::push_fuzzy_match(
                    &mut suggestions,
                    &full_command,
                    self.matcher.fuzzy_match(&alias_kebab, input),
                    5,
                );
            }
        }

        // Sort by score and take top results
        suggestions.sort_by_key(|b| std::cmp::Reverse(b.1));
        suggestions.truncate(max_results);

        suggestions
    }

    fn push_fuzzy_match(
        suggestions: &mut Vec<(String, i64)>,
        command: &str,
        score: Option<i64>,
        bonus: i64,
    ) {
        let Some(score) = score.filter(|score| *score > 0) else {
            return;
        };
        suggestions.push((command.to_string(), score + bonus));
    }
}

impl Default for CommandSearcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Intent matching uses whole Unicode alphanumeric tokens, never fuzzy text
/// assembled across fields. A leading HTTP method qualifies multiword queries.
fn score_intent(command: &CachedCommand, query: &str, operation_name: &str) -> ScoringResult {
    let mut words = query.split_whitespace();
    let first = words.next().unwrap_or_default();
    let qualified = is_http_method(first);
    if qualified && !command.method.eq_ignore_ascii_case(first) {
        return ScoringResult::default();
    }
    let intent = if qualified {
        words.collect::<Vec<_>>().join(" ")
    } else {
        query.to_string()
    };
    let tokens = keyword_tokens(&intent);
    if tokens.is_empty() {
        return ScoringResult::default();
    }
    let aliases = command.aliases.join(" ");
    // Search the same word boundaries users see in generated command paths.
    let display_name = to_kebab_case(command.display_name.as_deref().unwrap_or(""));
    let group = to_kebab_case(command.display_group.as_deref().unwrap_or(&command.name));
    let fields = [
        ("Operation", operation_name, 300),
        ("Summary", command.summary.as_deref().unwrap_or(""), 200),
        (
            "Description",
            command.description.as_deref().unwrap_or(""),
            100,
        ),
        ("Path", command.path.as_str(), 150),
        ("Display name", display_name.as_str(), 300),
        ("Group", group.as_str(), 150),
        ("Alias", aliases.as_str(), 300),
    ];
    let tokenized: Vec<_> = fields
        .iter()
        .map(|(_, text, _)| keyword_tokens(text))
        .collect();
    if !tokens
        .iter()
        .all(|token| tokenized.iter().any(|field| field.contains(token)))
    {
        return ScoringResult::default();
    }
    let mut result = score_intent_fields(&fields, &tokenized, &tokens);
    result.score += name_similarity_score(
        &normalize_keyword(operation_name),
        &normalize_keyword(&intent),
    );
    result
}

/// Field coherence supplies a bounded bonus, independent of alias count.
fn score_intent_fields(
    fields: &[(&str, &str, i64)],
    tokenized: &[Vec<String>],
    tokens: &[String],
) -> ScoringResult {
    let mut result = ScoringResult {
        score: 80,
        highlights: Vec::new(),
    };
    for ((label, text, bonus), field) in fields.iter().zip(tokenized) {
        if tokens.iter().all(|token| field.contains(token)) {
            let extra = field.len().saturating_sub(tokens.len()).min(20);
            result.score = result.score.max(bonus - i64::try_from(extra).unwrap_or(20));
            result.highlights.push(format!("{label}: {text}"));
        }
    }
    result
}

fn is_http_method(word: &str) -> bool {
    [
        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
    ]
    .iter()
    .any(|method| word.eq_ignore_ascii_case(method))
}

fn keyword_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Stable tie breakers make ranking independent of specification operation order.
fn compare_search_results(a: &CommandSearchResult, b: &CommandSearchResult) -> std::cmp::Ordering {
    b.score
        .cmp(&a.score)
        .then_with(|| a.api_context.cmp(&b.api_context))
        .then_with(|| a.command_path.cmp(&b.command_path))
        .then_with(|| a.command.operation_id.cmp(&b.command.operation_id))
}

/// Regex matching is explicit so valid ordinary words do not disable fuzzy search.
fn compile_search_regex(query: &str) -> Result<Option<Regex>, Error> {
    query
        .strip_prefix("regex:")
        .map(Regex::new)
        .transpose()
        .map_err(|error| Error::validation_error(format!("Invalid search regex: {error}")))
}

/// Ignore punctuation and case when comparing operation names.
fn normalize_keyword(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Name matches outrank incidental matches in long descriptions.
fn name_similarity_score(name: &str, query: &str) -> i64 {
    if query.is_empty() {
        return 0;
    }
    if name == query {
        return 1_000;
    }
    if strsim::damerau_levenshtein(name, query) <= 1 && query.chars().count() >= 4 {
        return 500;
    }
    0
}

/// Returns the effective command path using display overrides if present.
///
/// Uses `command.name` (not `tags.first()`) for the group fallback to stay
/// consistent with `engine::generator::effective_group_name`.
fn effective_command_path(command: &CachedCommand) -> String {
    let group = command.display_group.as_ref().map_or_else(
        || {
            if command.name.is_empty() {
                constants::DEFAULT_GROUP.to_string()
            } else {
                to_kebab_case(&command.name)
            }
        },
        |g| to_kebab_case(g),
    );
    let name = command.display_name.as_ref().map_or_else(
        || {
            if command.operation_id.is_empty() {
                command.method.to_lowercase()
            } else {
                to_kebab_case(&command.operation_id)
            }
        },
        |n| to_kebab_case(n),
    );
    format!("{group} {name}")
}

fn format_param_flag(p: &CachedParameter) -> String {
    let required = if p.required { "*" } else { "" };
    format!("--{}{}", to_kebab_case(&p.name), required)
}

/// Format search results for display
#[must_use]
pub fn format_search_results(results: &[CommandSearchResult], verbose: bool) -> Vec<String> {
    format_search_results_with_style(results, verbose, DiscoveryStyle::new(false))
}

/// Format search results for display with optional semantic styling.
#[must_use]
pub fn format_search_results_with_style(
    results: &[CommandSearchResult],
    verbose: bool,
    style: DiscoveryStyle,
) -> Vec<String> {
    let mut lines = Vec::new();

    if results.is_empty() {
        lines.push("No matching operations found.".to_string());
        lines.push(
            "Try broader terms or run `aperture commands <api>` to browse by structure."
                .to_string(),
        );
        return lines;
    }

    lines.push(format!(
        "{} {} matching operation(s):",
        style.heading("Found"),
        results.len()
    ));
    lines.push(String::new());

    for (idx, result) in results.iter().enumerate() {
        let number = idx + 1;

        // Basic result line
        lines.push(format!(
            "{}. aperture api {} {}",
            number, result.api_context, result.command_path
        ));

        // Method and path
        lines.push(format!(
            "   {} {}",
            style.method(&result.command.method),
            style.metadata(&result.command.path)
        ));

        // Description if available
        if let Some(ref summary) = result.command.summary {
            lines.push(format!("   {summary}"));
        }

        lines.push(format!(
            "   {} aperture docs {} {}",
            style.next_label("Inspect:"),
            result.api_context,
            result.command_path
        ));
        lines.push(format!(
            "   {} aperture api {} {} ...",
            style.next_label("Execute:"),
            result.api_context,
            result.command_path
        ));

        if !verbose {
            lines.push(String::new());
            continue;
        }

        // Show highlights
        if !result.highlights.is_empty() {
            lines.push(format!(
                "   {} {}",
                style.next_label("Matches:"),
                result.highlights.join(", ")
            ));
        }

        // Show parameters
        if !result.command.parameters.is_empty() {
            let params: Vec<String> = result
                .command
                .parameters
                .iter()
                .map(format_param_flag)
                .collect();
            lines.push(format!(
                "   {} {}",
                style.next_label("Parameters:"),
                params.join(" ")
            ));
        }

        // Show request body if present
        if result.command.request_body.is_some() {
            lines.push("   Request body: JSON required".to_string());
        }

        lines.push(String::new());
    }

    lines
}
