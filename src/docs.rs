//! Documentation and help system for improved CLI discoverability

use crate::cache::models::{CachedCommand, CachedParameter, CachedSpec, CommandExample};
use crate::constants;
use crate::discovery_style::DiscoveryStyle;
use crate::error::Error;
use crate::utils::to_kebab_case;
use std::collections::BTreeMap;
use std::fmt::Write;

/// Documentation generator for API operations
pub struct DocumentationGenerator {
    specs: BTreeMap<String, CachedSpec>,
}

impl DocumentationGenerator {
    /// Create a new documentation generator
    #[must_use]
    pub const fn new(specs: BTreeMap<String, CachedSpec>) -> Self {
        Self { specs }
    }

    /// Generate comprehensive help for a specific command
    /// Generate command help documentation
    ///
    /// # Errors
    /// Returns an error if the API or operation is not found
    pub fn generate_command_help(
        &self,
        api_name: &str,
        tag: &str,
        operation_id: &str,
    ) -> Result<String, Error> {
        self.generate_command_help_styled(api_name, tag, operation_id, DiscoveryStyle::new(false))
    }

    /// Generate command help documentation with optional semantic styling.
    ///
    /// # Errors
    /// Returns an error if the API or operation is not found
    pub fn generate_command_help_styled(
        &self,
        api_name: &str,
        tag: &str,
        operation_id: &str,
        style: DiscoveryStyle,
    ) -> Result<String, Error> {
        let spec = self
            .specs
            .get(api_name)
            .ok_or_else(|| Error::spec_not_found(api_name))?;

        let command = spec
            .commands
            .iter()
            .find(|cmd| Self::matches_command_reference(cmd, tag, operation_id))
            .ok_or_else(|| {
                Error::spec_not_found(format!(
                    "Operation '{tag} {operation_id}' not found in API '{api_name}'"
                ))
            })?;

        let mut help = String::new();

        // Build help sections
        Self::add_command_header(&mut help, command, style);
        Self::add_usage_section(&mut help, api_name, command, style);
        Self::add_parameters_section(&mut help, command, style);
        Self::add_request_body_section(&mut help, command, style);
        Self::add_examples_section(&mut help, api_name, command, style);
        Self::add_responses_section(&mut help, command, style);
        Self::add_authentication_section(&mut help, command, style);
        Self::add_metadata_section(&mut help, command, style);

        Ok(help)
    }

    /// Returns the effective command group shown in CLI paths.
    pub(crate) fn effective_group(command: &CachedCommand) -> String {
        command.display_group.as_ref().map_or_else(
            || {
                if command.name.is_empty() {
                    constants::DEFAULT_GROUP.to_string()
                } else {
                    to_kebab_case(&command.name)
                }
            },
            |group| to_kebab_case(group),
        )
    }

    /// Returns the effective command name shown in CLI paths.
    pub(crate) fn effective_operation(command: &CachedCommand) -> String {
        command.display_name.as_ref().map_or_else(
            || {
                if command.operation_id.is_empty() {
                    command.method.to_lowercase()
                } else {
                    to_kebab_case(&command.operation_id)
                }
            },
            |name| to_kebab_case(name),
        )
    }

    /// Returns true when the provided docs path references this command.
    ///
    /// Supports both effective (mapped) names and original names for compatibility.
    pub(crate) fn matches_command_reference(
        command: &CachedCommand,
        tag: &str,
        operation: &str,
    ) -> bool {
        let requested_tag = to_kebab_case(tag);
        let requested_operation = to_kebab_case(operation);

        let effective_tag = Self::effective_group(command);
        let effective_operation = Self::effective_operation(command);
        let (legacy_tag, legacy_operation) = Self::legacy_command_reference(command);

        let tag_match = requested_tag == effective_tag || requested_tag == legacy_tag;
        let operation_match = requested_operation == effective_operation
            || requested_operation == legacy_operation
            || Self::is_operation_alias_match(command, &requested_operation);

        tag_match && operation_match
    }

    fn legacy_command_reference(command: &CachedCommand) -> (String, String) {
        let legacy_tag = command.tags.first().map_or_else(
            || {
                if command.name.is_empty() {
                    constants::DEFAULT_GROUP.to_string()
                } else {
                    to_kebab_case(&command.name)
                }
            },
            |tag| to_kebab_case(tag),
        );

        let legacy_operation = if command.operation_id.is_empty() {
            command.method.to_lowercase()
        } else {
            to_kebab_case(&command.operation_id)
        };

        (legacy_tag, legacy_operation)
    }

    fn is_operation_alias_match(command: &CachedCommand, requested_operation: &str) -> bool {
        command
            .aliases
            .iter()
            .any(|alias| to_kebab_case(alias) == requested_operation)
    }

    /// Add command header with title and description
    fn add_command_header(help: &mut String, command: &CachedCommand, style: DiscoveryStyle) {
        write!(
            help,
            "# {} {}\n\n",
            style.method(&command.method),
            command.path
        )
        .ok();

        if let Some(summary) = &command.summary {
            write!(help, "**{summary}**\n\n").ok();
        }

        if let Some(description) = &command.description {
            write!(help, "{description}\n\n").ok();
        }
    }

    /// Add usage section with command syntax
    fn add_usage_section(
        help: &mut String,
        api_name: &str,
        command: &CachedCommand,
        style: DiscoveryStyle,
    ) {
        writeln!(help, "## {}\n", style.heading("Usage")).ok();
        write!(
            help,
            "```bash\n{}\n```\n\n",
            Self::canonical_usage(api_name, command)
        )
        .ok();
    }

    /// Builds canonical usage output from the effective runtime command model.
    #[must_use]
    pub(crate) fn canonical_usage(api_name: &str, command: &CachedCommand) -> String {
        let mut usage = Self::base_command(api_name, command);

        for param in &command.parameters {
            if !param.required {
                continue;
            }
            usage.push(' ');
            usage.push_str(&Self::required_parameter_usage_fragment(param));
        }

        if command
            .request_body
            .as_ref()
            .is_some_and(|body| body.required)
        {
            usage.push_str(" --body '{\"key\": \"value\"}'");
        }

        usage
    }

    /// Add parameters section if parameters exist
    fn add_parameters_section(help: &mut String, command: &CachedCommand, style: DiscoveryStyle) {
        if command.parameters.is_empty() {
            return;
        }

        writeln!(help, "## {}\n", style.heading("Parameters")).ok();
        for param in &command.parameters {
            let required_badge = if param.required {
                format!(" {}", style.required("**(required)**"))
            } else {
                String::new()
            };
            let param_type = param.schema_type.as_deref().unwrap_or("string");
            writeln!(
                help,
                "- `--{}` ({}){}  - {}",
                to_kebab_case(&param.name),
                param_type,
                required_badge,
                param.description.as_deref().unwrap_or("No description")
            )
            .ok();
        }
        help.push('\n');
    }

    /// Add request body section if present
    fn add_request_body_section(help: &mut String, command: &CachedCommand, style: DiscoveryStyle) {
        let Some(ref body) = command.request_body else {
            return;
        };

        writeln!(help, "## {}\n", style.heading("Request Body")).ok();
        if let Some(ref description) = body.description {
            write!(help, "{description}\n\n").ok();
        }
        write!(help, "Required: {}\n\n", body.required).ok();
    }

    /// Add examples section with command examples
    fn add_examples_section(
        help: &mut String,
        api_name: &str,
        command: &CachedCommand,
        style: DiscoveryStyle,
    ) {
        let examples = Self::canonical_examples(api_name, command);

        if examples.len() > 1 {
            writeln!(help, "## {}\n", style.heading("Examples")).ok();
        } else {
            writeln!(help, "## {}\n", style.heading("Example")).ok();
        }

        for (i, example) in examples.iter().enumerate() {
            if examples.len() > 1 {
                write!(help, "### Example {}\n\n", i + 1).ok();
            }
            write!(help, "**{}**\n\n", example.description).ok();
            if let Some(ref explanation) = example.explanation {
                write!(help, "{explanation}\n\n").ok();
            }
            write!(help, "```bash\n{}\n```\n\n", example.command_line).ok();
        }
    }

    /// Builds canonical examples from the effective runtime command model.
    #[must_use]
    pub(crate) fn canonical_examples(
        api_name: &str,
        command: &CachedCommand,
    ) -> Vec<CommandExample> {
        let base_cmd = Self::base_command(api_name, command);
        let required_params: Vec<&CachedParameter> =
            command.parameters.iter().filter(|p| p.required).collect();
        let optional_query_params: Vec<&CachedParameter> = command
            .parameters
            .iter()
            .filter(|p| !p.required && p.location == "query")
            .take(2)
            .collect();

        let mut examples = Vec::new();
        Self::push_required_parameters_example(&mut examples, &base_cmd, command, &required_params);
        Self::push_request_body_example(&mut examples, &base_cmd, command, &required_params);
        Self::push_optional_parameters_example(
            &mut examples,
            &base_cmd,
            &required_params,
            &optional_query_params,
        );

        if examples.is_empty() {
            examples.push(Self::build_basic_example(&base_cmd, command));
        }

        examples
    }

    fn push_required_parameters_example(
        examples: &mut Vec<CommandExample>,
        base_cmd: &str,
        command: &CachedCommand,
        required_params: &[&CachedParameter],
    ) {
        if required_params.is_empty() {
            return;
        }

        let mut command_line = base_cmd.to_string();
        for param in required_params {
            Self::append_example_parameter(&mut command_line, param, "example");
        }
        examples.push(CommandExample {
            description: "Basic usage with required parameters".to_string(),
            command_line,
            explanation: Some(format!("{} {}", command.method, command.path)),
        });
    }

    fn push_request_body_example(
        examples: &mut Vec<CommandExample>,
        base_cmd: &str,
        command: &CachedCommand,
        required_params: &[&CachedParameter],
    ) {
        if command.request_body.is_none() {
            return;
        }

        let mut command_line = base_cmd.to_string();
        for param in required_params {
            Self::append_example_parameter(&mut command_line, param, "example");
        }
        command_line.push_str(" --body '{\"name\": \"example\", \"value\": 42}'");

        examples.push(CommandExample {
            description: "With request body".to_string(),
            command_line,
            explanation: Some("Sends JSON data in the request body".to_string()),
        });
    }

    fn push_optional_parameters_example(
        examples: &mut Vec<CommandExample>,
        base_cmd: &str,
        required_params: &[&CachedParameter],
        optional_query_params: &[&CachedParameter],
    ) {
        if required_params.is_empty() || optional_query_params.is_empty() {
            return;
        }

        let mut command_line = base_cmd.to_string();
        for param in required_params {
            Self::append_example_parameter(&mut command_line, param, "value");
        }
        for param in optional_query_params {
            Self::append_example_parameter(&mut command_line, param, "optional");
        }

        examples.push(CommandExample {
            description: "With optional parameters".to_string(),
            command_line,
            explanation: Some(
                "Includes optional query parameters for filtering or customization".to_string(),
            ),
        });
    }

    fn build_basic_example(base_cmd: &str, command: &CachedCommand) -> CommandExample {
        CommandExample {
            description: "Basic usage".to_string(),
            command_line: base_cmd.to_string(),
            explanation: Some(format!("Executes {} {}", command.method, command.path)),
        }
    }

    fn base_command(api_name: &str, command: &CachedCommand) -> String {
        let group = Self::effective_group(command);
        let operation = Self::effective_operation(command);
        format!("aperture api {api_name} {group} {operation}")
    }

    fn required_parameter_usage_fragment(param: &CachedParameter) -> String {
        let flag = format!("--{}", to_kebab_case(&param.name));
        if Self::is_boolean_parameter(param) {
            return flag;
        }

        let placeholder = to_kebab_case(&param.name).replace('-', "_").to_uppercase();
        format!("{flag} <{placeholder}>")
    }

    fn append_example_parameter(
        command_line: &mut String,
        param: &CachedParameter,
        fallback: &str,
    ) {
        write!(command_line, " --{}", to_kebab_case(&param.name)).ok();

        if Self::is_boolean_parameter(param) {
            return;
        }

        let value = param
            .example
            .as_deref()
            .unwrap_or_else(|| Self::default_example_value(param.schema_type.as_deref(), fallback));
        write!(command_line, " {value}").ok();
    }

    fn default_example_value<'a>(schema_type: Option<&'a str>, fallback: &'a str) -> &'a str {
        match schema_type {
            Some("integer" | "number") => "123",
            Some("array") => "[item1,item2]",
            Some("object") => "{\"key\":\"value\"}",
            _ => fallback,
        }
    }

    fn is_boolean_parameter(param: &CachedParameter) -> bool {
        param.schema_type.as_ref().is_some_and(|t| t == "boolean")
    }

    /// Add responses section if responses exist
    fn add_responses_section(help: &mut String, command: &CachedCommand, style: DiscoveryStyle) {
        if !command.responses.is_empty() {
            writeln!(help, "## {}\n", style.heading("Responses")).ok();
            for response in &command.responses {
                writeln!(
                    help,
                    "- **{}**: {}",
                    response.status_code,
                    response.description.as_deref().unwrap_or("No description")
                )
                .ok();
            }
            help.push('\n');
        }
    }

    /// Add authentication section if security requirements exist
    fn add_authentication_section(
        help: &mut String,
        command: &CachedCommand,
        style: DiscoveryStyle,
    ) {
        if !command.security_requirements.is_empty() {
            writeln!(help, "## {}\n", style.heading("Authentication")).ok();
            help.push_str("This operation requires authentication. Available schemes:\n\n");
            for scheme_name in &command.security_requirements {
                writeln!(help, "- {scheme_name}").ok();
            }
            help.push('\n');
        }
    }

    /// Add metadata section with deprecation and external docs
    fn add_metadata_section(help: &mut String, command: &CachedCommand, style: DiscoveryStyle) {
        if command.deprecated {
            writeln!(
                help,
                "{}\n",
                style.warning("Deprecated: this operation is deprecated")
            )
            .ok();
        }

        if let Some(ref docs_url) = command.external_docs_url {
            write!(help, "External documentation: {docs_url}\n\n").ok();
        }
    }

    /// Generate API overview with statistics
    ///
    /// # Errors
    /// Returns an error if the API is not found
    pub fn generate_api_overview(&self, api_name: &str) -> Result<String, Error> {
        self.generate_api_overview_styled(api_name, DiscoveryStyle::new(false))
    }

    /// Generate API overview with statistics and optional semantic styling.
    ///
    /// # Errors
    /// Returns an error if the API is not found
    pub fn generate_api_overview_styled(
        &self,
        api_name: &str,
        style: DiscoveryStyle,
    ) -> Result<String, Error> {
        let spec = self
            .specs
            .get(api_name)
            .ok_or_else(|| Error::spec_not_found(api_name))?;

        let visible_commands: Vec<&CachedCommand> = spec
            .commands
            .iter()
            .filter(|command| !command.hidden)
            .collect();

        let mut overview = String::new();
        Self::write_api_overview_header(&mut overview, spec, style);
        Self::write_api_overview_statistics(&mut overview, &visible_commands, style);
        Self::write_api_overview_quick_start(&mut overview, api_name, style);
        Self::write_api_overview_samples(&mut overview, api_name, &visible_commands, style);

        Ok(overview)
    }

    /// Generate an API reference index for docs navigation.
    ///
    /// # Errors
    /// Returns an error if the API is not found
    pub fn generate_api_reference_index(&self, api_name: &str) -> Result<String, Error> {
        self.generate_api_reference_index_styled(api_name, DiscoveryStyle::new(false))
    }

    /// Generate API reference index with optional semantic styling.
    ///
    /// # Errors
    /// Returns an error if the API is not found
    pub fn generate_api_reference_index_styled(
        &self,
        api_name: &str,
        style: DiscoveryStyle,
    ) -> Result<String, Error> {
        let spec = self
            .specs
            .get(api_name)
            .ok_or_else(|| Error::spec_not_found(api_name))?;

        let visible_commands: Vec<&CachedCommand> = spec
            .commands
            .iter()
            .filter(|command| !command.hidden)
            .collect();

        let mut category_counts = BTreeMap::new();
        for command in &visible_commands {
            *category_counts
                .entry(Self::effective_group(command))
                .or_insert(0usize) += 1;
        }

        let mut reference = String::new();
        Self::write_api_reference_header(&mut reference, spec, style);
        Self::write_api_reference_navigation(&mut reference, api_name, style);
        Self::write_api_reference_categories(&mut reference, &category_counts, style);
        Self::write_api_reference_examples(&mut reference, api_name, &visible_commands, style);

        Ok(reference)
    }

    fn write_api_overview_header(overview: &mut String, spec: &CachedSpec, style: DiscoveryStyle) {
        writeln!(overview, "# {} API", style.api_title(&spec.name)).ok();
        overview.push('\n');
        writeln!(
            overview,
            "{}: {}",
            style.metadata("**Version**"),
            spec.version
        )
        .ok();

        if let Some(base_url) = spec.base_url.as_deref() {
            writeln!(overview, "{}: {base_url}", style.metadata("**Base URL**")).ok();
        }
        overview.push('\n');
    }

    fn write_api_overview_statistics(
        overview: &mut String,
        visible_commands: &[&CachedCommand],
        style: DiscoveryStyle,
    ) {
        let mut method_counts = BTreeMap::new();
        let mut tag_counts = BTreeMap::new();

        for command in visible_commands {
            *method_counts.entry(command.method.clone()).or_insert(0) += 1;
            *tag_counts
                .entry(Self::effective_group(command))
                .or_insert(0) += 1;
        }

        writeln!(overview, "## {}\n", style.heading("Statistics")).ok();
        writeln!(
            overview,
            "- {}: {}",
            style.muted_count("**Total Operations**"),
            visible_commands.len()
        )
        .ok();
        writeln!(overview, "- {}:", style.metadata("**Methods**")).ok();
        for (method, count) in method_counts {
            writeln!(overview, "  - {}: {count}", style.method(&method)).ok();
        }
        writeln!(overview, "- {}:", style.metadata("**Categories**")).ok();
        for (tag, count) in tag_counts {
            writeln!(overview, "  - {tag}: {count}").ok();
        }
        overview.push('\n');
    }

    fn write_api_overview_quick_start(
        overview: &mut String,
        api_name: &str,
        style: DiscoveryStyle,
    ) {
        writeln!(overview, "## {}\n", style.heading("Quick Start")).ok();
        write!(
            overview,
            "List all available commands:\n```bash\naperture commands {api_name}\n```\n\n"
        )
        .ok();

        write!(
            overview,
            "Search for specific operations:\n```bash\naperture search \"keyword\" --api {api_name}\n```\n\n"
        )
        .ok();
    }

    fn write_api_overview_samples(
        overview: &mut String,
        api_name: &str,
        visible_commands: &[&CachedCommand],
        style: DiscoveryStyle,
    ) {
        if visible_commands.is_empty() {
            return;
        }

        writeln!(overview, "## {}\n", style.heading("Sample Operations")).ok();
        for (i, command) in visible_commands.iter().take(3).enumerate() {
            let tag = Self::effective_group(command);
            let operation = Self::effective_operation(command);
            write!(
                overview,
                "{}. **{}** ({})\n   ```bash\n   aperture api {api_name} {tag} {operation}\n   ```\n   {}\n\n",
                i + 1,
                command.summary.as_deref().unwrap_or(&operation),
                style.method(&command.method),
                command.description.as_deref().unwrap_or("No description")
            )
            .ok();
        }
    }

    fn write_api_reference_header(
        reference: &mut String,
        spec: &CachedSpec,
        style: DiscoveryStyle,
    ) {
        writeln!(reference, "# {} API Reference", style.api_title(&spec.name)).ok();
        reference.push('\n');
        writeln!(
            reference,
            "{}: {}",
            style.metadata("**Version**"),
            spec.version
        )
        .ok();
        if let Some(base_url) = spec.base_url.as_deref() {
            writeln!(reference, "{}: {base_url}", style.metadata("**Base URL**")).ok();
        }
        reference.push('\n');
        reference.push_str(
            "Use this view to inspect operation-level documentation before executing commands.\n\n",
        );
    }

    fn write_api_reference_navigation(
        reference: &mut String,
        api_name: &str,
        style: DiscoveryStyle,
    ) {
        writeln!(reference, "## {}\n", style.heading("Reference Workflow")).ok();
        write!(
            reference,
            "1. Find operations by intent:\n```bash\naperture search \"keyword\" --api {api_name}\n```\n\n"
        )
        .ok();
        write!(
            reference,
            "2. Inspect command structure:\n```bash\naperture commands {api_name}\n```\n\n"
        )
        .ok();
        write!(
            reference,
            "3. Open deep operation docs:\n```bash\naperture docs {api_name} <tag> <operation>\n```\n\n"
        )
        .ok();
    }

    fn write_api_reference_categories(
        reference: &mut String,
        category_counts: &BTreeMap<String, usize>,
        style: DiscoveryStyle,
    ) {
        writeln!(reference, "## {}\n", style.heading("Categories")).ok();

        if category_counts.is_empty() {
            reference.push_str("No visible operations found.\n\n");
            return;
        }

        for (category, count) in category_counts {
            writeln!(reference, "- `{category}` ({count} operations)").ok();
        }
        reference.push('\n');
    }

    fn write_api_reference_examples(
        reference: &mut String,
        api_name: &str,
        visible_commands: &[&CachedCommand],
        style: DiscoveryStyle,
    ) {
        if visible_commands.is_empty() {
            return;
        }

        writeln!(reference, "## {}\n", style.heading("Example Docs Paths")).ok();
        for command in visible_commands.iter().take(3) {
            let tag = Self::effective_group(command);
            let operation = Self::effective_operation(command);
            let summary = command
                .summary
                .as_deref()
                .or(command.description.as_deref())
                .unwrap_or("No description");
            writeln!(
                reference,
                "- `aperture docs {api_name} {tag} {operation}` — {summary}"
            )
            .ok();
        }
        reference.push('\n');
    }

    /// Generate interactive help menu
    #[must_use]
    pub fn generate_interactive_menu(&self) -> String {
        let mut menu = String::new();

        menu.push_str("# Aperture Interactive Help\n\n");
        menu.push_str("Welcome to Aperture! Here are some ways to get started:\n\n");

        // Available APIs
        if self.specs.is_empty() {
            menu.push_str("## No APIs Configured\n\n");
            menu.push_str("Get started by adding an API specification:\n");
            menu.push_str("```bash\naperture config api add myapi ./openapi.yaml\n```\n\n");
        } else {
            menu.push_str("## Your APIs\n\n");
            for (api_name, spec) in &self.specs {
                let operation_count = spec.commands.len();
                writeln!(
                    menu,
                    "- **{api_name}** ({operation_count} operations) - Version {}",
                    spec.version
                )
                .ok();
            }
            menu.push('\n');
        }

        // Common commands
        menu.push_str("## Common Commands\n\n");
        menu.push_str("- `aperture config api list` - List all configured APIs\n");
        menu.push_str("- `aperture search <term>` - Search across all APIs\n");
        menu.push_str("- `aperture commands <api>` - Show available commands for an API\n");
        menu.push_str("- `aperture run <shortcut>` - Execute using shortcuts\n");
        menu.push_str("- `aperture api <api> --help` - Get help for an API\n\n");

        // Tips
        menu.push_str("## Tips\n\n");
        menu.push_str("- Use `--describe-json` for machine-readable capability information\n");
        menu.push_str("- Use `--dry-run` to see what request would be made without executing\n");
        menu.push_str("- Use `--json-errors` for structured error output\n");
        menu.push_str("- Environment variables can be used for authentication (see config)\n\n");

        menu
    }
}

/// Enhanced help formatter for better readability
pub struct HelpFormatter;

impl HelpFormatter {
    /// Format command list with enhanced styling
    #[must_use]
    pub fn format_command_list(spec: &CachedSpec) -> String {
        Self::format_command_list_with_style(spec, DiscoveryStyle::new(false))
    }

    /// Format command list with optional semantic styling.
    #[must_use]
    pub fn format_command_list_with_style(spec: &CachedSpec, style: DiscoveryStyle) -> String {
        let mut output = String::new();

        // Header with API info
        writeln!(output, "{} API Commands", style.api_title(&spec.name)).ok();
        let visible_commands: Vec<&CachedCommand> = spec
            .commands
            .iter()
            .filter(|command| !command.hidden)
            .collect();

        writeln!(
            output,
            "   {}: {} | {}: {}",
            style.metadata("Version"),
            spec.version,
            style.metadata("Operations"),
            visible_commands.len()
        )
        .ok();

        if let Some(ref base_url) = spec.base_url {
            writeln!(output, "   {}: {base_url}", style.metadata("Base URL")).ok();
        }
        output.push_str(&"═".repeat(60));
        output.push('\n');

        // Group by effective command group
        let mut tag_groups = BTreeMap::new();
        for command in visible_commands {
            let tag = DocumentationGenerator::effective_group(command);
            tag_groups.entry(tag).or_insert_with(Vec::new).push(command);
        }

        for (tag, commands) in tag_groups {
            writeln!(output, "\n{}: {}", style.heading("Group"), tag).ok();
            output.push_str(&"─".repeat(40));
            output.push('\n');

            for command in commands {
                let operation_kebab = DocumentationGenerator::effective_operation(command);
                let method_badge = Self::format_method_badge(&command.method, style);
                let description = command
                    .summary
                    .as_ref()
                    .or(command.description.as_ref())
                    .map(|s| format!(" - {}", s.lines().next().unwrap_or(s)))
                    .unwrap_or_default();

                writeln!(
                    output,
                    "  {} {} {}{}",
                    method_badge,
                    operation_kebab,
                    if command.deprecated { "Deprecated" } else { "" },
                    description
                )
                .ok();

                // Show path as subdued text
                writeln!(output, "     {}: {}", style.metadata("Path"), command.path).ok();
            }
        }

        output.push('\n');
        output
    }

    /// Format HTTP method for display.
    fn format_method_badge(method: &str, style: DiscoveryStyle) -> String {
        format!("{:<7}", style.method(method))
    }
}
