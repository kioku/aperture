//! Command shortcuts and aliases for improved CLI usability

use crate::cache::models::{CachedCommand, CachedSpec};
use crate::utils::to_kebab_case;
use std::collections::{BTreeMap, HashMap};

/// Represents a resolved command shortcut
#[derive(Debug, Clone)]
pub struct ResolvedShortcut {
    /// The full command path that should be executed
    pub full_command: Vec<String>,
    /// The spec containing the command
    pub spec: CachedSpec,
    /// The resolved command
    pub command: CachedCommand,
    /// Confidence score (0-100, higher is better)
    pub confidence: u8,
}

/// Command resolution result
#[derive(Debug)]
pub enum ResolutionResult {
    /// Exact match found
    Resolved(Box<ResolvedShortcut>),
    /// Multiple possible matches
    Ambiguous(Vec<ResolvedShortcut>),
    /// No matches found
    NotFound,
}

/// Command shortcut resolver
#[allow(clippy::struct_field_names)]
pub struct ShortcutResolver {
    /// Map of operation IDs to specs and commands
    operation_map: HashMap<String, Vec<(String, CachedSpec, CachedCommand)>>,
    /// Map of HTTP method + path combinations
    method_path_map: HashMap<String, Vec<(String, CachedSpec, CachedCommand)>>,
    /// Map of tag-based shortcuts
    tag_map: HashMap<String, Vec<(String, CachedSpec, CachedCommand)>>,
}

impl ShortcutResolver {
    /// Create a new shortcut resolver
    #[must_use]
    pub fn new() -> Self {
        Self {
            operation_map: HashMap::new(),
            method_path_map: HashMap::new(),
            tag_map: HashMap::new(),
        }
    }

    /// Index all available commands for shortcut resolution
    pub fn index_specs(&mut self, specs: &BTreeMap<String, CachedSpec>) {
        // Clear existing indexes
        self.operation_map.clear();
        self.method_path_map.clear();
        self.tag_map.clear();

        for (api_name, spec) in specs {
            for command in &spec.commands {
                // Index by operation ID (both original and kebab-case)
                let operation_kebab = to_kebab_case(&command.operation_id);

                // Original operation ID
                if !command.operation_id.is_empty() {
                    self.operation_map
                        .entry(command.operation_id.clone())
                        .or_default()
                        .push((api_name.clone(), spec.clone(), command.clone()));
                }

                // Kebab-case operation ID
                if operation_kebab != command.operation_id {
                    self.operation_map
                        .entry(operation_kebab.clone())
                        .or_default()
                        .push((api_name.clone(), spec.clone(), command.clone()));
                }

                // Index by HTTP method + path
                let method = command.method.to_uppercase();
                let path = &command.path;
                let method_path_key = format!("{method} {path}");
                self.method_path_map
                    .entry(method_path_key)
                    .or_default()
                    .push((api_name.clone(), spec.clone(), command.clone()));

                // Index by tags
                for tag in &command.tags {
                    let tag_key = to_kebab_case(tag);
                    self.tag_map.entry(tag_key.clone()).or_default().push((
                        api_name.clone(),
                        spec.clone(),
                        command.clone(),
                    ));

                    // Also index tag + operation combinations
                    let tag_operation_key = format!("{tag_key} {operation_kebab}");
                    self.tag_map.entry(tag_operation_key).or_default().push((
                        api_name.clone(),
                        spec.clone(),
                        command.clone(),
                    ));
                }
            }
        }
    }

    /// Resolve a command shortcut to full command path
    ///
    /// # Panics
    ///
    /// Panics if candidates is empty when exactly one match is expected.
    /// This should not happen in practice due to the length check.
    #[must_use]
    pub fn resolve_shortcut(&self, args: &[String]) -> ResolutionResult {
        if args.is_empty() {
            return ResolutionResult::NotFound;
        }

        let mut candidates = Vec::new();

        // Try different resolution strategies in order of preference

        // 1. Direct operation ID match
        if let Some(matches) = self.try_operation_id_resolution(args) {
            candidates.extend(matches);
        }

        // 2. HTTP method + path resolution
        if let Some(matches) = self.try_method_path_resolution(args) {
            candidates.extend(matches);
        }

        // 3. Tag-based resolution
        if let Some(matches) = self.try_tag_resolution(args) {
            candidates.extend(matches);
        }

        // 4. Partial matching (fuzzy) - only if no candidates found yet
        if candidates.is_empty() {
            candidates.extend(self.try_partial_matching(args).unwrap_or_default());
        }

        match candidates.len() {
            0 => ResolutionResult::NotFound,
            1 => {
                // Handle the single candidate case safely
                candidates.into_iter().next().map_or_else(
                    || {
                        // This should never happen given len() == 1, but handle defensively
                        // ast-grep-ignore: no-println
                        eprintln!("Warning: Expected exactly one candidate but found none");
                        ResolutionResult::NotFound
                    },
                    |candidate| ResolutionResult::Resolved(Box::new(candidate)),
                )
            }
            _ => {
                // Sort by confidence score (descending)
                candidates.sort_by(|a, b| b.confidence.cmp(&a.confidence));

                // Check if the top candidate has significantly higher confidence
                let has_high_confidence = candidates[0].confidence >= 85
                    && (candidates.len() == 1
                        || candidates[0].confidence > candidates[1].confidence + 10);

                if !has_high_confidence {
                    return ResolutionResult::Ambiguous(candidates);
                }

                // Handle the high-confidence candidate case safely
                candidates.into_iter().next().map_or_else(
                    || {
                        // This should never happen given we just accessed candidates[0], but handle defensively
                        // ast-grep-ignore: no-println
                        eprintln!("Warning: Expected candidates after sorting but found none");
                        ResolutionResult::NotFound
                    },
                    |candidate| ResolutionResult::Resolved(Box::new(candidate)),
                )
            }
        }
    }

    /// Try to resolve using direct operation ID matching
    fn try_operation_id_resolution(&self, args: &[String]) -> Option<Vec<ResolvedShortcut>> {
        let operation_id = &args[0];

        self.operation_map.get(operation_id).map(|matches| {
            matches
                .iter()
                .map(|(api_name, spec, command)| {
                    let tag = command
                        .tags
                        .first()
                        .map_or_else(|| "api".to_string(), |t| to_kebab_case(t));
                    let operation_kebab = to_kebab_case(&command.operation_id);

                    ResolvedShortcut {
                        full_command: vec![
                            "api".to_string(),
                            api_name.clone(),
                            tag,
                            operation_kebab,
                        ],
                        spec: spec.clone(),
                        command: command.clone(),
                        confidence: 95, // High confidence for exact operation ID match
                    }
                })
                .collect()
        })
    }

    /// Try to resolve using HTTP method + path
    fn try_method_path_resolution(&self, args: &[String]) -> Option<Vec<ResolvedShortcut>> {
        if args.len() < 2 {
            return None;
        }

        let method = args[0].to_uppercase();
        let path = &args[1];
        let method_path_key = format!("{method} {path}");

        self.method_path_map.get(&method_path_key).map(|matches| {
            matches
                .iter()
                .map(|(api_name, spec, command)| {
                    let tag = command
                        .tags
                        .first()
                        .map_or_else(|| "api".to_string(), |t| to_kebab_case(t));
                    let operation_kebab = to_kebab_case(&command.operation_id);

                    ResolvedShortcut {
                        full_command: vec![
                            "api".to_string(),
                            api_name.clone(),
                            tag,
                            operation_kebab,
                        ],
                        spec: spec.clone(),
                        command: command.clone(),
                        confidence: 90, // High confidence for exact method+path match
                    }
                })
                .collect()
        })
    }

    /// Try to resolve using tag-based matching
    fn try_tag_resolution(&self, args: &[String]) -> Option<Vec<ResolvedShortcut>> {
        let mut candidates = Vec::new();

        // Try single tag lookup - convert to kebab-case for matching
        let tag_kebab = to_kebab_case(&args[0]);
        if let Some(matches) = self.tag_map.get(&tag_kebab) {
            for (api_name, spec, command) in matches {
                let tag = command
                    .tags
                    .first()
                    .map_or_else(|| "api".to_string(), |t| to_kebab_case(t));
                let operation_kebab = to_kebab_case(&command.operation_id);

                candidates.push(ResolvedShortcut {
                    full_command: vec!["api".to_string(), api_name.clone(), tag, operation_kebab],
                    spec: spec.clone(),
                    command: command.clone(),
                    confidence: 70, // Medium confidence for tag-only match
                });
            }
        }

        // Try tag + operation combination if we have 2+ args
        if args.len() < 2 {
            return if candidates.is_empty() {
                None
            } else {
                Some(candidates)
            };
        }

        let tag = to_kebab_case(&args[0]);
        let operation = to_kebab_case(&args[1]);
        let tag_operation_key = format!("{tag} {operation}");

        if let Some(matches) = self.tag_map.get(&tag_operation_key) {
            for (api_name, spec, command) in matches {
                let tag = command
                    .tags
                    .first()
                    .map_or_else(|| "api".to_string(), |t| to_kebab_case(t));
                let operation_kebab = to_kebab_case(&command.operation_id);

                candidates.push(ResolvedShortcut {
                    full_command: vec!["api".to_string(), api_name.clone(), tag, operation_kebab],
                    spec: spec.clone(),
                    command: command.clone(),
                    confidence: 85, // Higher confidence for tag+operation match
                });
            }
        }

        if candidates.is_empty() {
            None
        } else {
            Some(candidates)
        }
    }

    /// Try partial matching using fuzzy logic
    fn try_partial_matching(&self, args: &[String]) -> Option<Vec<ResolvedShortcut>> {
        use fuzzy_matcher::skim::SkimMatcherV2;
        use fuzzy_matcher::FuzzyMatcher;

        let matcher = SkimMatcherV2::default().ignore_case();
        let query = args.join(" ");
        let mut candidates = Vec::new();

        // Search through operation IDs
        for (operation_id, matches) in &self.operation_map {
            if let Some(score) = matcher.fuzzy_match(operation_id, &query) {
                for (api_name, spec, command) in matches {
                    let tag = command
                        .tags
                        .first()
                        .map_or_else(|| "api".to_string(), |t| to_kebab_case(t));
                    let operation_kebab = to_kebab_case(&command.operation_id);

                    candidates.push(ResolvedShortcut {
                        full_command: vec![
                            "api".to_string(),
                            api_name.clone(),
                            tag,
                            operation_kebab,
                        ],
                        spec: spec.clone(),
                        command: command.clone(),
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        confidence: std::cmp::min(60, (score / 10).max(20) as u8), // Scale fuzzy score
                    });
                }
            }
        }

        if candidates.is_empty() {
            None
        } else {
            Some(candidates)
        }
    }

    /// Generate suggestions for ambiguous matches
    #[must_use]
    pub fn format_ambiguous_suggestions(&self, matches: &[ResolvedShortcut]) -> String {
        let mut suggestions = Vec::new();

        for (i, shortcut) in matches.iter().take(5).enumerate() {
            let cmd = shortcut.full_command.join(" ");
            let desc = shortcut
                .command
                .description
                .as_deref()
                .unwrap_or("No description");
            let num = i + 1;
            suggestions.push(format!("{num}. aperture {cmd} - {desc}"));
        }

        format!(
            "Multiple commands match. Did you mean:\n{}",
            suggestions.join("\n")
        )
    }
}

impl Default for ShortcutResolver {
    fn default() -> Self {
        Self::new()
    }
}
