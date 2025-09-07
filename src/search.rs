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
            // Apply API filter if specified
            if let Some(filter) = api_filter {
                if api_name != filter {
                    continue;
                }
            }

            for command in &spec.commands {
                let mut highlights = Vec::new();
                let mut total_score = 0i64;

                // Build searchable text from command attributes with pre-allocated capacity
                let operation_id_kebab = to_kebab_case(&command.operation_id);
                let summary = command.summary.as_deref().unwrap_or("");
                let description = command.description.as_deref().unwrap_or("");

                // Pre-calculate capacity to avoid multiple allocations
                let capacity = operation_id_kebab.len()
                    + command.operation_id.len()
                    + command.method.len()
                    + command.path.len()
                    + summary.len()
                    + description.len()
                    + 6; // 6 spaces

                let mut search_text = String::with_capacity(capacity);
                search_text.push_str(&operation_id_kebab);
                search_text.push(' ');
                search_text.push_str(&command.operation_id);
                search_text.push(' ');
                search_text.push_str(&command.method);
                search_text.push(' ');
                search_text.push_str(&command.path);
                search_text.push(' ');
                search_text.push_str(summary);
                search_text.push(' ');
                search_text.push_str(description);

                // Score based on different matching strategies
                if let Some(ref regex) = regex_pattern {
                    if regex.is_match(&search_text) {
                        // Dynamic scoring based on match quality for regex
                        let base_score = 90;
                        let query_len = regex.as_str().len();
                        #[allow(clippy::cast_possible_wrap)]
                        let match_specificity_bonus = query_len.min(10) as i64;
                        total_score = base_score + match_specificity_bonus;
                        highlights.push(format!("Regex match: {}", regex.as_str()));
                    }
                } else {
                    // Fuzzy match on complete search text
                    if let Some(score) = self.matcher.fuzzy_match(&search_text, query) {
                        total_score += score;
                    }

                    // Bonus score for exact substring matches
                    let query_lower = query.to_lowercase();
                    if operation_id_kebab.to_lowercase().contains(&query_lower) {
                        total_score += 50;
                        highlights.push(format!("Operation: {operation_id_kebab}"));
                    }
                    // Also check original operation ID
                    if command.operation_id.to_lowercase().contains(&query_lower) {
                        total_score += 50;
                        highlights.push(format!("Operation: {}", command.operation_id));
                    }
                    if command.method.to_lowercase().contains(&query_lower) {
                        total_score += 30;
                        highlights.push(format!("Method: {}", command.method));
                    }
                    if command.path.to_lowercase().contains(&query_lower) {
                        total_score += 20;
                        highlights.push(format!("Path: {}", command.path));
                    }
                    if let Some(ref summary) = command.summary {
                        if summary.to_lowercase().contains(&query_lower) {
                            total_score += 15;
                            highlights.push("Summary match".to_string());
                        }
                    }
                }

                // Only include results with positive scores
                if total_score > 0 {
                    let tag = command
                        .tags
                        .first()
                        .map_or_else(|| constants::DEFAULT_GROUP.to_string(), Clone::clone);
                    let command_path = format!("{} {}", tag.to_lowercase(), operation_id_kebab);

                    results.push(CommandSearchResult {
                        api_context: api_name.clone(),
                        command: command.clone(),
                        command_path,
                        score: total_score,
                        highlights,
                    });
                }
            }
        }

        // Sort by score (highest first)
        results.sort_by(|a, b| b.score.cmp(&a.score));

        Ok(results)
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
            let tag = command
                .tags
                .first()
                .map_or_else(|| constants::DEFAULT_GROUP.to_string(), Clone::clone);
            let full_command = format!("{} {}", tag.to_lowercase(), operation_id_kebab);

            // Check fuzzy match score
            if let Some(score) = self.matcher.fuzzy_match(&full_command, input) {
                if score > 0 {
                    suggestions.push((full_command.clone(), score));
                }
            }

            // Also check just the operation ID
            if let Some(score) = self.matcher.fuzzy_match(&operation_id_kebab, input) {
                if score > 0 {
                    suggestions.push((full_command, score + 10)); // Bonus for direct match
                }
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

        if verbose {
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
        }

        lines.push(String::new());
    }

    lines
}
