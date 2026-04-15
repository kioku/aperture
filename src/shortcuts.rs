//! Command shortcuts and aliases for improved CLI usability

use crate::cache::models::{CachedCommand, CachedSpec};
use crate::constants;
use crate::utils::to_kebab_case;
use std::collections::{BTreeMap, HashMap};

/// Builds the full command path for a resolved shortcut, using effective
/// display names when command mappings are active.
fn build_full_command(api_name: &str, command: &CachedCommand) -> Vec<String> {
    // Use `command.name` (not `tags.first()`) for consistency with
    // `engine::generator::effective_group_name` and `search::effective_command_path`.
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
    let operation = command.display_name.as_ref().map_or_else(
        || {
            if command.operation_id.is_empty() {
                command.method.to_lowercase()
            } else {
                to_kebab_case(&command.operation_id)
            }
        },
        |n| to_kebab_case(n),
    );
    vec!["api".to_string(), api_name.to_string(), group, operation]
}

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
        self.clear_indexes();

        for (api_name, spec) in specs {
            for command in &spec.commands {
                self.index_single_command(api_name, spec, command);
            }
        }
    }

    fn clear_indexes(&mut self) {
        self.operation_map.clear();
        self.method_path_map.clear();
        self.tag_map.clear();
    }

    fn index_single_command(&mut self, api_name: &str, spec: &CachedSpec, command: &CachedCommand) {
        let operation_kebab = to_kebab_case(&command.operation_id);
        self.index_operation_identifiers(api_name, spec, command, &operation_kebab);
        self.index_method_path(api_name, spec, command);
        self.index_display_and_aliases(api_name, spec, command);
        self.index_tags(api_name, spec, command, &operation_kebab);
    }

    fn index_operation_identifiers(
        &mut self,
        api_name: &str,
        spec: &CachedSpec,
        command: &CachedCommand,
        operation_kebab: &str,
    ) {
        if !command.operation_id.is_empty() {
            self.push_operation_entry(&command.operation_id, api_name, spec, command);
        }

        if operation_kebab != command.operation_id {
            self.push_operation_entry(operation_kebab, api_name, spec, command);
        }
    }

    fn index_method_path(&mut self, api_name: &str, spec: &CachedSpec, command: &CachedCommand) {
        let method_path_key = format!("{} {}", command.method.to_uppercase(), command.path);
        self.method_path_map
            .entry(method_path_key)
            .or_default()
            .push((api_name.to_string(), spec.clone(), command.clone()));
    }

    fn index_display_and_aliases(
        &mut self,
        api_name: &str,
        spec: &CachedSpec,
        command: &CachedCommand,
    ) {
        if let Some(display_name) = command.display_name.as_deref() {
            self.push_operation_entry(&to_kebab_case(display_name), api_name, spec, command);
        }

        for alias in &command.aliases {
            self.push_operation_entry(&to_kebab_case(alias), api_name, spec, command);
        }
    }

    fn index_tags(
        &mut self,
        api_name: &str,
        spec: &CachedSpec,
        command: &CachedCommand,
        operation_kebab: &str,
    ) {
        let mut effective_tags: Vec<String> =
            command.tags.iter().map(|tag| to_kebab_case(tag)).collect();
        if let Some(display_group) = command.display_group.as_deref() {
            effective_tags.push(to_kebab_case(display_group));
        }

        let effective_name = command
            .display_name
            .as_deref()
            .map_or_else(|| operation_kebab.to_string(), to_kebab_case);

        for tag_key in effective_tags {
            self.push_tag_entry(&tag_key, api_name, spec, command);
            self.push_tag_entry(
                &format!("{tag_key} {effective_name}"),
                api_name,
                spec,
                command,
            );
        }
    }

    fn push_operation_entry(
        &mut self,
        key: &str,
        api_name: &str,
        spec: &CachedSpec,
        command: &CachedCommand,
    ) {
        self.operation_map
            .entry(key.to_string())
            .or_default()
            .push((api_name.to_string(), spec.clone(), command.clone()));
    }

    fn push_tag_entry(
        &mut self,
        key: &str,
        api_name: &str,
        spec: &CachedSpec,
        command: &CachedCommand,
    ) {
        self.tag_map.entry(key.to_string()).or_default().push((
            api_name.to_string(),
            spec.clone(),
            command.clone(),
        ));
    }

    /// Resolve a command shortcut to full command path
    ///
    /// # Panics
    ///
    /// Panics if candidates is empty when exactly one match is expected.
    /// This should not happen in practice due to the length check.
    fn collect_resolution_candidates(&self, args: &[String]) -> Vec<ResolvedShortcut> {
        let mut candidates = Vec::new();

        if let Some(matches) = self.try_operation_id_resolution(args) {
            candidates.extend(matches);
        }

        if let Some(matches) = self.try_method_path_resolution(args) {
            candidates.extend(matches);
        }

        if let Some(matches) = self.try_tag_resolution(args) {
            candidates.extend(matches);
        }

        if candidates.is_empty() {
            candidates.extend(self.try_partial_matching(args).unwrap_or_default());
        }

        candidates
    }

    fn resolve_from_candidates(candidates: Vec<ResolvedShortcut>) -> ResolutionResult {
        match candidates.len() {
            0 => ResolutionResult::NotFound,
            1 => Self::resolve_single_candidate(candidates),
            _ => Self::resolve_best_candidate(candidates),
        }
    }

    fn deduplicate_candidates(candidates: Vec<ResolvedShortcut>) -> Vec<ResolvedShortcut> {
        let mut deduped: Vec<ResolvedShortcut> = Vec::new();
        let mut seen_indexes: HashMap<Vec<String>, usize> = HashMap::new();

        for candidate in candidates {
            match seen_indexes.get(&candidate.full_command).copied() {
                Some(existing_index)
                    if candidate.confidence > deduped[existing_index].confidence =>
                {
                    deduped[existing_index] = candidate;
                }
                Some(_) => {}
                None => {
                    seen_indexes.insert(candidate.full_command.clone(), deduped.len());
                    deduped.push(candidate);
                }
            }
        }

        deduped
    }

    fn sort_candidates_by_confidence(candidates: &mut [ResolvedShortcut]) {
        candidates.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| a.full_command.cmp(&b.full_command))
                .then_with(|| a.command.operation_id.cmp(&b.command.operation_id))
                .then_with(|| a.command.method.cmp(&b.command.method))
                .then_with(|| a.command.path.cmp(&b.command.path))
        });
    }

    fn resolve_single_candidate(candidates: Vec<ResolvedShortcut>) -> ResolutionResult {
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

    fn resolve_best_candidate(mut candidates: Vec<ResolvedShortcut>) -> ResolutionResult {
        Self::sort_candidates_by_confidence(&mut candidates);

        if !Self::has_high_confidence_candidate(&candidates) {
            return ResolutionResult::Ambiguous(candidates);
        }

        Self::resolve_single_candidate(candidates)
    }

    fn has_high_confidence_candidate(candidates: &[ResolvedShortcut]) -> bool {
        candidates[0].confidence >= 85
            && (candidates.len() == 1 || candidates[0].confidence > candidates[1].confidence + 10)
    }

    #[must_use]
    pub fn resolve_shortcut(&self, args: &[String]) -> ResolutionResult {
        if args.is_empty() {
            return ResolutionResult::NotFound;
        }

        let candidates = Self::deduplicate_candidates(self.collect_resolution_candidates(args));
        Self::resolve_from_candidates(candidates)
    }

    /// Try to resolve using direct operation ID matching
    fn try_operation_id_resolution(&self, args: &[String]) -> Option<Vec<ResolvedShortcut>> {
        let operation_id = &args[0];

        self.operation_map.get(operation_id).map(|matches| {
            matches
                .iter()
                .map(|(api_name, spec, command)| ResolvedShortcut {
                    full_command: build_full_command(api_name, command),
                    spec: spec.clone(),
                    command: command.clone(),
                    confidence: 95, // High confidence for exact operation ID match
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
                .map(|(api_name, spec, command)| ResolvedShortcut {
                    full_command: build_full_command(api_name, command),
                    spec: spec.clone(),
                    command: command.clone(),
                    confidence: 90, // High confidence for exact method+path match
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
                candidates.push(ResolvedShortcut {
                    full_command: build_full_command(api_name, command),
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
                candidates.push(ResolvedShortcut {
                    full_command: build_full_command(api_name, command),
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
                    candidates.push(ResolvedShortcut {
                        full_command: build_full_command(api_name, command),
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
