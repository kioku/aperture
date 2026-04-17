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

        // Try to compile as regex first
        let regex_pattern = Regex::new(query).ok();

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
        results.sort_by_key(|b| std::cmp::Reverse(b.score));

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
            || self.score_with_fuzzy_match(command, query, &search_text, &operation_id_kebab),
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
        search_text: &str,
        operation_id_kebab: &str,
    ) -> ScoringResult {
        let mut highlights = Vec::new();
        let mut total_score = 0i64;

        // Fuzzy match on complete search text
        if let Some(score) = self.matcher.fuzzy_match(search_text, query) {
            total_score += score;
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

        // Bonus for alias matches
        for alias in &command.aliases {
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
