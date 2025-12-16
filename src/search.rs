//! Command search functionality for discovering API operations
//!
//! This module provides search capabilities to help users find relevant
//! API operations across registered specifications using fuzzy matching
//! and keyword search.

use crate::cache::models::{CachedCommand, CachedSpec};
use crate::constants;
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
                    let operation_id_kebab = to_kebab_case(&command.operation_id);
                    let tag = command.tags.first().map_or_else(
                        || constants::DEFAULT_GROUP.to_string(),
                        |t| to_kebab_case(t),
                    );
                    let command_path = format!("{tag} {operation_id_kebab}");

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
        results.sort_by(|a, b| b.score.cmp(&a.score));

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

        // Build searchable text from command attributes
        let search_text = format!(
            "{operation_id_kebab} {} {} {} {summary} {description}",
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
            let operation_id_kebab = to_kebab_case(&command.operation_id);
            let tag = command.tags.first().map_or_else(
                || constants::DEFAULT_GROUP.to_string(),
                |t| to_kebab_case(t),
            );
            let full_command = format!("{tag} {operation_id_kebab}");

            // Check fuzzy match score - use match with guard to avoid nesting
            match self.matcher.fuzzy_match(&full_command, input) {
                Some(score) if score > 0 => suggestions.push((full_command.clone(), score)),
                _ => {}
            }

            // Also check just the operation ID - use match with guard to avoid nesting
            match self.matcher.fuzzy_match(&operation_id_kebab, input) {
                Some(score) if score > 0 => suggestions.push((full_command.clone(), score + 10)), // Bonus for direct match
                _ => {}
            }
        }

        // Sort by score and take top results
        suggestions.sort_by(|a, b| b.1.cmp(&a.1));
        suggestions.truncate(max_results);

        suggestions
    }
}

impl Default for CommandSearcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Format search results for display
#[must_use]
pub fn format_search_results(results: &[CommandSearchResult], verbose: bool) -> Vec<String> {
    let mut lines = Vec::new();

    if results.is_empty() {
        lines.push("No matching operations found.".to_string());
        return lines;
    }

    lines.push(format!("Found {} matching operation(s):", results.len()));
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
            result.command.method.to_uppercase(),
            result.command.path
        ));

        // Description if available
        if let Some(ref summary) = result.command.summary {
            lines.push(format!("   {summary}"));
        }

        if !verbose {
            lines.push(String::new());
            continue;
        }

        // Show highlights
        if !result.highlights.is_empty() {
            lines.push(format!("   Matches: {}", result.highlights.join(", ")));
        }

        // Show parameters
        if !result.command.parameters.is_empty() {
            let params: Vec<String> = result
                .command
                .parameters
                .iter()
                .map(|p| {
                    let required = if p.required { "*" } else { "" };
                    format!("--{}{}", p.name, required)
                })
                .collect();
            lines.push(format!("   Parameters: {}", params.join(" ")));
        }

        // Show request body if present
        if result.command.request_body.is_some() {
            lines.push("   Request body: JSON required".to_string());
        }

        lines.push(String::new());
    }

    lines
}
