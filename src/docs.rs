//! Documentation and help system for improved CLI discoverability

use crate::cache::models::{CachedCommand, CachedSpec};
use crate::constants;
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

        let effective_tag = Self::effective_group(command);
        let effective_operation = Self::effective_operation(command);

        let mut help = String::new();

        // Build help sections
        Self::add_command_header(&mut help, command);
        Self::add_usage_section(&mut help, api_name, &effective_tag, &effective_operation);
        Self::add_parameters_section(&mut help, command);
        Self::add_request_body_section(&mut help, command);
        Self::add_examples_section(
            &mut help,
            api_name,
            &effective_tag,
            &effective_operation,
            command,
        );
        Self::add_responses_section(&mut help, command);
        Self::add_authentication_section(&mut help, command);
        Self::add_metadata_section(&mut help, command);

        Ok(help)
    }

    /// Returns the effective command group shown in CLI paths.
    fn effective_group(command: &CachedCommand) -> String {
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
    fn effective_operation(command: &CachedCommand) -> String {
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
    fn matches_command_reference(command: &CachedCommand, tag: &str, operation: &str) -> bool {
        let requested_tag = to_kebab_case(tag);
        let requested_operation = to_kebab_case(operation);

        let effective_tag = Self::effective_group(command);
        let effective_operation = Self::effective_operation(command);

        let legacy_tag = command.tags.first().map_or_else(
            || {
                if command.name.is_empty() {
                    constants::DEFAULT_GROUP.to_string()
                } else {
                    to_kebab_case(&command.name)
                }
            },
            |t| to_kebab_case(t),
        );

        let legacy_operation = if command.operation_id.is_empty() {
            command.method.to_lowercase()
        } else {
            to_kebab_case(&command.operation_id)
        };

        let alias_match = command
            .aliases
            .iter()
            .any(|alias| to_kebab_case(alias) == requested_operation);

        let tag_match = requested_tag == effective_tag || requested_tag == legacy_tag;
        let operation_match = requested_operation == effective_operation
            || requested_operation == legacy_operation
            || alias_match;

        tag_match && operation_match
    }

    /// Add command header with title and description
    fn add_command_header(help: &mut String, command: &CachedCommand) {
        write!(
            help,
            "# {} {}\n\n",
            command.method.to_uppercase(),
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
    fn add_usage_section(help: &mut String, api_name: &str, tag: &str, operation_id: &str) {
        help.push_str("## Usage\n\n");
        write!(
            help,
            "```bash\naperture api {api_name} {tag} {operation_id}\n```\n\n"
        )
        .ok();
    }

    /// Add parameters section if parameters exist
    fn add_parameters_section(help: &mut String, command: &CachedCommand) {
        if command.parameters.is_empty() {
            return;
        }

        help.push_str("## Parameters\n\n");
        for param in &command.parameters {
            let required_badge = if param.required {
                " **(required)**"
            } else {
                ""
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
    fn add_request_body_section(help: &mut String, command: &CachedCommand) {
        let Some(ref body) = command.request_body else {
            return;
        };

        help.push_str("## Request Body\n\n");
        if let Some(ref description) = body.description {
            write!(help, "{description}\n\n").ok();
        }
        write!(help, "Required: {}\n\n", body.required).ok();
    }

    /// Add examples section with command examples
    fn add_examples_section(
        help: &mut String,
        api_name: &str,
        tag: &str,
        operation_id: &str,
        command: &CachedCommand,
    ) {
        if command.examples.is_empty() {
            help.push_str("## Example\n\n");
            help.push_str(&Self::generate_basic_example(
                api_name,
                tag,
                operation_id,
                command,
            ));
            return;
        }

        help.push_str("## Examples\n\n");
        for (i, example) in command.examples.iter().enumerate() {
            write!(help, "### Example {}\n\n", i + 1).ok();
            write!(help, "**{}**\n\n", example.description).ok();
            if let Some(ref explanation) = example.explanation {
                write!(help, "{explanation}\n\n").ok();
            }
            write!(help, "```bash\n{}\n```\n\n", example.command_line).ok();
        }
    }

    /// Add responses section if responses exist
    fn add_responses_section(help: &mut String, command: &CachedCommand) {
        if !command.responses.is_empty() {
            help.push_str("## Responses\n\n");
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
    fn add_authentication_section(help: &mut String, command: &CachedCommand) {
        if !command.security_requirements.is_empty() {
            help.push_str("## Authentication\n\n");
            help.push_str("This operation requires authentication. Available schemes:\n\n");
            for scheme_name in &command.security_requirements {
                writeln!(help, "- {scheme_name}").ok();
            }
            help.push('\n');
        }
    }

    /// Add metadata section with deprecation and external docs
    fn add_metadata_section(help: &mut String, command: &CachedCommand) {
        if command.deprecated {
            help.push_str("⚠️  **This operation is deprecated**\n\n");
        }

        if let Some(ref docs_url) = command.external_docs_url {
            write!(help, "📖 **External Documentation**: {docs_url}\n\n").ok();
        }
    }

    /// Generate a basic example for a command
    fn generate_basic_example(
        api_name: &str,
        tag: &str,
        operation_id: &str,
        command: &CachedCommand,
    ) -> String {
        let mut example = format!("```bash\naperture api {api_name} {tag} {operation_id}");

        // Add required parameters
        for param in &command.parameters {
            if param.required {
                let param_type = param.schema_type.as_deref().unwrap_or("string");
                let example_value = Self::generate_example_value(param_type);
                write!(
                    example,
                    " --{} {}",
                    to_kebab_case(&param.name),
                    example_value
                )
                .ok();
            }
        }

        // Add request body if required
        match command.request_body {
            Some(ref body) if body.required => {
                example.push_str(" --body '{\"key\": \"value\"}'");
            }
            _ => {}
        }

        example.push_str("\n```\n\n");
        example
    }

    /// Generate example values for different parameter types
    fn generate_example_value(param_type: &str) -> &'static str {
        match param_type.to_lowercase().as_str() {
            "string" => "\"example\"",
            "integer" | "number" => "123",
            "boolean" => "true",
            "array" => "[\"item1\",\"item2\"]",
            _ => "\"value\"",
        }
    }

    /// Generate API overview with statistics
    ///
    /// # Errors
    /// Returns an error if the API is not found
    pub fn generate_api_overview(&self, api_name: &str) -> Result<String, Error> {
        let spec = self
            .specs
            .get(api_name)
            .ok_or_else(|| Error::spec_not_found(api_name))?;

        let mut overview = String::new();

        // API header
        write!(overview, "# {} API\n\n", spec.name).ok();
        writeln!(overview, "**Version**: {}", spec.version).ok();

        if let Some(ref base_url) = spec.base_url {
            writeln!(overview, "**Base URL**: {base_url}").ok();
        }
        overview.push('\n');

        // Statistics (hidden commands are excluded from docs-style listings)
        let visible_commands: Vec<&CachedCommand> = spec
            .commands
            .iter()
            .filter(|command| !command.hidden)
            .collect();
        let total_operations = visible_commands.len();
        let mut method_counts = BTreeMap::new();
        let mut tag_counts = BTreeMap::new();

        for command in &visible_commands {
            *method_counts.entry(command.method.clone()).or_insert(0) += 1;
            *tag_counts
                .entry(Self::effective_group(command))
                .or_insert(0) += 1;
        }

        overview.push_str("## Statistics\n\n");
        writeln!(overview, "- **Total Operations**: {total_operations}").ok();
        overview.push_str("- **Methods**:\n");
        for (method, count) in method_counts {
            writeln!(overview, "  - {method}: {count}").ok();
        }
        overview.push_str("- **Categories**:\n");
        for (tag, count) in tag_counts {
            writeln!(overview, "  - {tag}: {count}").ok();
        }
        overview.push('\n');

        // Quick start examples
        overview.push_str("## Quick Start\n\n");
        write!(
            overview,
            "List all available commands:\n```bash\naperture list-commands {api_name}\n```\n\n"
        )
        .ok();

        write!(
            overview,
            "Search for specific operations:\n```bash\naperture search \"keyword\" --api {api_name}\n```\n\n"
        ).ok();

        // Show first few operations as examples
        if !visible_commands.is_empty() {
            overview.push_str("## Sample Operations\n\n");
            for (i, command) in visible_commands.iter().take(3).enumerate() {
                let tag = Self::effective_group(command);
                let operation = Self::effective_operation(command);
                write!(
                    overview,
                    "{}. **{}** ({})\n   ```bash\n   aperture api {api_name} {tag} {operation}\n   ```\n   {}\n\n",
                    i + 1,
                    command.summary.as_deref().unwrap_or(&operation),
                    command.method.to_uppercase(),
                    command.description.as_deref().unwrap_or("No description")
                )
                .ok();
            }
        }

        Ok(overview)
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
            menu.push_str("```bash\naperture config add myapi ./openapi.yaml\n```\n\n");
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
        menu.push_str("- `aperture config list` - List all configured APIs\n");
        menu.push_str("- `aperture search <term>` - Search across all APIs\n");
        menu.push_str("- `aperture list-commands <api>` - Show available commands for an API\n");
        menu.push_str("- `aperture exec <shortcut>` - Execute using shortcuts\n");
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
        let mut output = String::new();

        // Header with API info
        writeln!(output, "📋 {} API Commands", spec.name).ok();
        let visible_commands: Vec<&CachedCommand> = spec
            .commands
            .iter()
            .filter(|command| !command.hidden)
            .collect();

        writeln!(
            output,
            "   Version: {} | Operations: {}",
            spec.version,
            visible_commands.len()
        )
        .ok();

        if let Some(ref base_url) = spec.base_url {
            writeln!(output, "   Base URL: {base_url}").ok();
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
            writeln!(output, "\n📁 {tag}").ok();
            output.push_str(&"─".repeat(40));
            output.push('\n');

            for command in commands {
                let operation_kebab = DocumentationGenerator::effective_operation(command);
                let method_badge = Self::format_method_badge(&command.method);
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
                    if command.deprecated { "⚠️" } else { "" },
                    description
                )
                .ok();

                // Show path as subdued text
                writeln!(output, "     Path: {}", command.path).ok();
            }
        }

        output.push('\n');
        output
    }

    /// Format HTTP method with color/styling
    fn format_method_badge(method: &str) -> String {
        match method.to_uppercase().as_str() {
            "GET" => "🔍 GET   ".to_string(),
            "POST" => "📝 POST  ".to_string(),
            "PUT" => "✏️  PUT   ".to_string(),
            "DELETE" => "🗑️  DELETE".to_string(),
            "PATCH" => "🔧 PATCH ".to_string(),
            "HEAD" => "👁️  HEAD  ".to_string(),
            "OPTIONS" => "⚙️  OPTIONS".to_string(),
            _ => format!("📋 {:<7}", method.to_uppercase()),
        }
    }
}
