//! Command mapping application logic.
//!
//! Applies user-defined command mappings (group renames, operation renames,
//! aliases, hidden flags) to cached commands after spec transformation.

use crate::cache::models::CachedCommand;
use crate::config::models::CommandMapping;
use crate::error::Error;
use crate::utils::to_kebab_case;
use std::collections::{HashMap, HashSet};

/// Names reserved for built-in Aperture commands that cannot be used as group names.
const RESERVED_GROUP_NAMES: &[&str] = &["config", "search", "exec", "docs", "overview"];

/// Result of applying command mappings, including any warnings.
#[derive(Debug)]
pub struct MappingResult {
    /// Warnings about stale or unused mappings
    pub warnings: Vec<String>,
}

/// Applies a `CommandMapping` to a list of cached commands.
///
/// For each command:
/// - If the command's first tag has a group mapping, sets `display_group`.
/// - If the command's `operation_id` has an operation mapping, sets
///   `display_name`, `display_group` (if specified), `aliases`, and `hidden`.
///
/// # Errors
///
/// Returns an error if the resulting mappings produce name collisions.
pub fn apply_command_mapping(
    commands: &mut [CachedCommand],
    mapping: &CommandMapping,
) -> Result<MappingResult, Error> {
    let mut warnings = Vec::new();

    // Track which mapping keys were actually used for stale detection
    let mut used_group_keys: HashSet<String> = HashSet::new();
    let mut used_operation_keys: HashSet<String> = HashSet::new();

    for command in commands.iter_mut() {
        // Apply group mapping based on the command's first tag
        let first_tag = command
            .tags
            .first()
            .map_or_else(|| command.name.clone(), Clone::clone);
        if let Some(display_group) = mapping.groups.get(first_tag.as_str()) {
            command.display_group = Some(display_group.clone());
            used_group_keys.insert(first_tag);
        }

        // Apply operation mapping based on operation_id
        let Some(op_mapping) = mapping.operations.get(&command.operation_id) else {
            continue;
        };
        used_operation_keys.insert(command.operation_id.clone());
        apply_operation_mapping(command, op_mapping);
    }

    // Detect stale group mappings
    for key in mapping.groups.keys() {
        if !used_group_keys.contains(key) {
            warnings.push(format!(
                "Command mapping: group mapping for tag '{key}' did not match any operations"
            ));
        }
    }

    // Detect stale operation mappings
    for key in mapping.operations.keys() {
        if !used_operation_keys.contains(key) {
            warnings.push(format!(
                "Command mapping: operation mapping for '{key}' did not match any operations"
            ));
        }
    }

    // Validate for collisions
    validate_no_collisions(commands)?;

    Ok(MappingResult { warnings })
}

/// Applies an individual operation mapping to a cached command.
fn apply_operation_mapping(
    command: &mut CachedCommand,
    op_mapping: &crate::config::models::OperationMapping,
) {
    command.display_name.clone_from(&op_mapping.name);
    // Operation-level group override takes precedence over tag-level
    if op_mapping.group.is_some() {
        command.display_group.clone_from(&op_mapping.group);
    }
    if !op_mapping.aliases.is_empty() {
        command.aliases.clone_from(&op_mapping.aliases);
    }
    command.hidden = op_mapping.hidden;
}

/// Resolves the effective group name for a command, considering display overrides.
fn effective_group(command: &CachedCommand) -> String {
    command
        .display_group
        .as_ref()
        .map_or_else(|| to_kebab_case(&command.name), |g| to_kebab_case(g))
}

/// Resolves the effective subcommand name for a command, considering display overrides.
fn effective_name(command: &CachedCommand) -> String {
    command.display_name.as_ref().map_or_else(
        || to_kebab_case(&command.operation_id),
        |n| to_kebab_case(n),
    )
}

/// Validates that no two commands resolve to the same (group, name) pair,
/// and that aliases don't collide with names or other aliases within the same group.
fn validate_no_collisions(commands: &[CachedCommand]) -> Result<(), Error> {
    // Map from (group, name) → operation_id for collision detection
    let mut seen: HashMap<(String, String), &str> = HashMap::new();

    for command in commands {
        let group = effective_group(command);
        let name = effective_name(command);

        // Check reserved group names
        if RESERVED_GROUP_NAMES.contains(&group.as_str()) {
            return Err(Error::invalid_config(format!(
                "Command mapping collision: group name '{group}' (from operation '{}') \
                 conflicts with built-in command '{group}'",
                command.operation_id
            )));
        }

        // Check primary name collision
        let key = (group.clone(), name.clone());
        if let Some(existing_op) = seen.get(&key) {
            return Err(Error::invalid_config(format!(
                "Command mapping collision: operations '{}' and '{}' both resolve to '{} {}'",
                existing_op, command.operation_id, key.0, key.1
            )));
        }
        seen.insert(key, &command.operation_id);

        // Check alias collisions within the same group
        for alias in &command.aliases {
            let alias_kebab = to_kebab_case(alias);
            let alias_key = (group.clone(), alias_kebab.clone());
            if let Some(existing_op) = seen.get(&alias_key) {
                return Err(Error::invalid_config(format!(
                    "Command mapping collision: alias '{alias_kebab}' for operation '{}' \
                     conflicts with '{}' in group '{group}'",
                    command.operation_id, existing_op
                )));
            }
            seen.insert(alias_key, &command.operation_id);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::models::CachedCommand;
    use crate::config::models::{CommandMapping, OperationMapping};
    use std::collections::HashMap;

    fn make_command(tag: &str, operation_id: &str) -> CachedCommand {
        CachedCommand {
            name: tag.to_string(),
            description: None,
            summary: None,
            operation_id: operation_id.to_string(),
            method: "GET".to_string(),
            path: format!("/{tag}"),
            parameters: vec![],
            request_body: None,
            responses: vec![],
            security_requirements: vec![],
            tags: vec![tag.to_string()],
            deprecated: false,
            external_docs_url: None,
            examples: vec![],
            display_group: None,
            display_name: None,
            aliases: vec![],
            hidden: false,
        }
    }

    #[test]
    fn test_apply_group_mapping() {
        let mut commands = vec![
            make_command("User Management", "getUser"),
            make_command("User Management", "createUser"),
        ];
        let mapping = CommandMapping {
            groups: HashMap::from([("User Management".to_string(), "users".to_string())]),
            operations: HashMap::new(),
        };

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(commands[0].display_group, Some("users".to_string()));
        assert_eq!(commands[1].display_group, Some("users".to_string()));
    }

    #[test]
    fn test_apply_operation_mapping() {
        let mut commands = vec![make_command("users", "getUserById")];
        let mapping = CommandMapping {
            groups: HashMap::new(),
            operations: HashMap::from([(
                "getUserById".to_string(),
                OperationMapping {
                    name: Some("fetch".to_string()),
                    group: Some("accounts".to_string()),
                    aliases: vec!["get".to_string(), "show".to_string()],
                    hidden: false,
                },
            )]),
        };

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(commands[0].display_name, Some("fetch".to_string()));
        assert_eq!(commands[0].display_group, Some("accounts".to_string()));
        assert_eq!(commands[0].aliases, vec!["get", "show"]);
    }

    #[test]
    fn test_hidden_operation() {
        let mut commands = vec![make_command("users", "deleteUser")];
        let mapping = CommandMapping {
            groups: HashMap::new(),
            operations: HashMap::from([(
                "deleteUser".to_string(),
                OperationMapping {
                    hidden: true,
                    ..Default::default()
                },
            )]),
        };

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert!(result.warnings.is_empty());
        assert!(commands[0].hidden);
    }

    #[test]
    fn test_stale_group_mapping_warns() {
        let mut commands = vec![make_command("users", "getUser")];
        let mapping = CommandMapping {
            groups: HashMap::from([("NonExistentTag".to_string(), "nope".to_string())]),
            operations: HashMap::new(),
        };

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("NonExistentTag"));
    }

    #[test]
    fn test_stale_operation_mapping_warns() {
        let mut commands = vec![make_command("users", "getUser")];
        let mapping = CommandMapping {
            groups: HashMap::new(),
            operations: HashMap::from([("nonExistentOp".to_string(), OperationMapping::default())]),
        };

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("nonExistentOp"));
    }

    #[test]
    fn test_collision_detection_same_name() {
        let mut commands = vec![
            make_command("users", "getUser"),
            make_command("users", "fetchUser"),
        ];
        let mapping = CommandMapping {
            groups: HashMap::new(),
            operations: HashMap::from([(
                "fetchUser".to_string(),
                OperationMapping {
                    name: Some("get-user".to_string()),
                    ..Default::default()
                },
            )]),
        };

        let result = apply_command_mapping(&mut commands, &mapping);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("collision"), "Error: {err_msg}");
    }

    #[test]
    fn test_collision_detection_alias_vs_name() {
        let mut commands = vec![
            make_command("users", "getUser"),
            make_command("users", "fetchUser"),
        ];
        let mapping = CommandMapping {
            groups: HashMap::new(),
            operations: HashMap::from([(
                "fetchUser".to_string(),
                OperationMapping {
                    aliases: vec!["get-user".to_string()],
                    ..Default::default()
                },
            )]),
        };

        let result = apply_command_mapping(&mut commands, &mapping);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("collision"), "Error: {err_msg}");
    }

    #[test]
    fn test_reserved_group_name_rejected() {
        let mut commands = vec![make_command("users", "getUser")];
        let mapping = CommandMapping {
            groups: HashMap::from([("users".to_string(), "config".to_string())]),
            operations: HashMap::new(),
        };

        let result = apply_command_mapping(&mut commands, &mapping);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("config"), "Error: {err_msg}");
    }

    #[test]
    fn test_operation_group_overrides_tag_group() {
        let mut commands = vec![make_command("User Management", "getUser")];
        let mapping = CommandMapping {
            groups: HashMap::from([("User Management".to_string(), "users".to_string())]),
            operations: HashMap::from([(
                "getUser".to_string(),
                OperationMapping {
                    group: Some("accounts".to_string()),
                    ..Default::default()
                },
            )]),
        };

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert!(result.warnings.is_empty());
        // Operation-level group override wins
        assert_eq!(commands[0].display_group, Some("accounts".to_string()));
    }

    #[test]
    fn test_no_mapping_leaves_commands_unchanged() {
        let mut commands = vec![make_command("users", "getUser")];
        let mapping = CommandMapping::default();

        let result = apply_command_mapping(&mut commands, &mapping).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(commands[0].display_group, None);
        assert_eq!(commands[0].display_name, None);
        assert!(commands[0].aliases.is_empty());
        assert!(!commands[0].hidden);
    }
}
