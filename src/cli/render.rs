//! Rendering layer for [`ExecutionResult`](crate::invocation::ExecutionResult) values.
//!
//! Converts structured execution results into user-facing output
//! (stdout) in the requested format (JSON, YAML, table). This module
//! owns all `println!` calls for API response rendering.

use crate::cache::models::CachedCommand;
use crate::cli::OutputFormat;
use crate::constants;
use crate::engine::executor::apply_jq_filter;
use crate::error::Error;
use crate::invocation::ExecutionResult;
use crate::utils::to_kebab_case;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;
use tabled::Table;

/// Maximum number of rows to display in table format to prevent memory exhaustion.
const MAX_TABLE_ROWS: usize = 1000;

// Table structures for tabled crate
#[derive(tabled::Tabled)]
struct TableRow {
    #[tabled(rename = "Key")]
    key: String,
    #[tabled(rename = "Value")]
    value: String,
}

#[derive(tabled::Tabled)]
struct KeyValue {
    #[tabled(rename = "Key")]
    key: String,
    #[tabled(rename = "Value")]
    value: String,
}

/// Renders an [`ExecutionResult`] to stdout in the given format.
///
/// # Errors
///
/// Returns an error if JQ filtering or serialization fails.
pub fn render_result(
    result: &ExecutionResult,
    format: &OutputFormat,
    jq_filter: Option<&str>,
) -> Result<(), Error> {
    match result {
        ExecutionResult::Success { body, .. } | ExecutionResult::Cached { body } => {
            if body.is_empty() {
                return Ok(());
            }
            format_and_print(body, format, jq_filter, false)?;
        }
        ExecutionResult::DryRun { request_info } => {
            let output = serde_json::to_string_pretty(request_info).map_err(|e| {
                Error::serialization_error(format!("Failed to serialize dry run info: {e}"))
            })?;
            // ast-grep-ignore: no-println
            println!("{output}");
        }
        ExecutionResult::Empty => {}
    }
    Ok(())
}

/// Renders an [`ExecutionResult`] to a `String` instead of stdout.
///
/// Used by the batch processor when capturing output.
///
/// # Errors
///
/// Returns an error if JQ filtering or serialization fails.
pub fn render_result_to_string(
    result: &ExecutionResult,
    format: &OutputFormat,
    jq_filter: Option<&str>,
) -> Result<Option<String>, Error> {
    match result {
        ExecutionResult::Success { body, .. } | ExecutionResult::Cached { body } => {
            if body.is_empty() {
                return Ok(None);
            }
            format_and_print(body, format, jq_filter, true)
        }
        ExecutionResult::DryRun { request_info } => {
            let output = serde_json::to_string_pretty(request_info).map_err(|e| {
                Error::serialization_error(format!("Failed to serialize dry run info: {e}"))
            })?;
            Ok(Some(output))
        }
        ExecutionResult::Empty => Ok(None),
    }
}

/// Renders extended examples for a command to stdout.
pub fn render_examples(operation: &CachedCommand) {
    // ast-grep-ignore: no-println
    println!("Command: {}\n", to_kebab_case(&operation.operation_id));

    if let Some(ref summary) = operation.summary {
        // ast-grep-ignore: no-println
        println!("Description: {summary}\n");
    }

    // ast-grep-ignore: no-println
    println!("Method: {} {}\n", operation.method, operation.path);

    if operation.examples.is_empty() {
        // ast-grep-ignore: no-println
        println!("No examples available for this command.");
        return;
    }

    // ast-grep-ignore: no-println
    println!("Examples:\n");
    for (i, example) in operation.examples.iter().enumerate() {
        // ast-grep-ignore: no-println
        println!("{}. {}", i + 1, example.description);
        // ast-grep-ignore: no-println
        println!("   {}", example.command_line);
        if let Some(ref explanation) = example.explanation {
            // ast-grep-ignore: no-println
            println!("   {explanation}");
        }
        // ast-grep-ignore: no-println
        println!();
    }

    // Additional helpful information
    if operation.parameters.is_empty() {
        return;
    }

    // ast-grep-ignore: no-println
    println!("Parameters:");
    for param in &operation.parameters {
        let required = if param.required { " (required)" } else { "" };
        let param_type = param.schema_type.as_deref().unwrap_or("string");
        // ast-grep-ignore: no-println
        println!("  --{}{} [{}]", param.name, required, param_type);

        let Some(ref desc) = param.description else {
            continue;
        };
        // ast-grep-ignore: no-println
        println!("      {desc}");
    }
    // ast-grep-ignore: no-println
    println!();

    if operation.request_body.is_some() {
        // ast-grep-ignore: no-println
        println!("Request Body:");
        // ast-grep-ignore: no-println
        println!("  --body JSON (required)");
        // ast-grep-ignore: no-println
        println!("      JSON data to send in the request body");
    }
}

// ── Internal helpers ────────────────────────────────────────────────

/// Core formatting logic shared by `render_result` and `render_result_to_string`.
fn format_and_print(
    response_text: &str,
    output_format: &OutputFormat,
    jq_filter: Option<&str>,
    capture_output: bool,
) -> Result<Option<String>, Error> {
    // Apply JQ filter if provided
    let processed_text = if let Some(filter) = jq_filter {
        apply_jq_filter(response_text, filter)?
    } else {
        response_text.to_string()
    };

    match output_format {
        OutputFormat::Json => {
            let output = serde_json::from_str::<Value>(&processed_text)
                .ok()
                .and_then(|json_value| serde_json::to_string_pretty(&json_value).ok())
                .unwrap_or_else(|| processed_text.clone());

            if capture_output {
                return Ok(Some(output));
            }
            // ast-grep-ignore: no-println
            println!("{output}");
        }
        OutputFormat::Yaml => {
            let output = serde_json::from_str::<Value>(&processed_text)
                .ok()
                .and_then(|json_value| serde_yaml::to_string(&json_value).ok())
                .unwrap_or_else(|| processed_text.clone());

            if capture_output {
                return Ok(Some(output));
            }
            // ast-grep-ignore: no-println
            println!("{output}");
        }
        OutputFormat::Table => {
            let Ok(json_value) = serde_json::from_str::<Value>(&processed_text) else {
                if capture_output {
                    return Ok(Some(processed_text));
                }
                // ast-grep-ignore: no-println
                println!("{processed_text}");
                return Ok(None);
            };

            let table_output = print_as_table(&json_value, capture_output)?;
            if capture_output {
                return Ok(table_output);
            }
        }
    }

    Ok(None)
}

/// Prints items as a numbered list.
fn print_numbered_list(items: &[Value], capture_output: bool) -> Option<String> {
    if capture_output {
        let mut output = String::new();
        for (i, item) in items.iter().enumerate() {
            writeln!(&mut output, "{}: {}", i, format_value_for_table(item))
                .expect("writing to String cannot fail");
        }
        return Some(output.trim_end().to_string());
    }

    for (i, item) in items.iter().enumerate() {
        // ast-grep-ignore: no-println
        println!("{}: {}", i, format_value_for_table(item));
    }
    None
}

/// Helper to output or capture a message.
fn output_or_capture(message: &str, capture_output: bool) -> Option<String> {
    if capture_output {
        return Some(message.to_string());
    }
    // ast-grep-ignore: no-println
    println!("{message}");
    None
}

/// Prints JSON data as a formatted table.
#[allow(clippy::unnecessary_wraps, clippy::too_many_lines)]
fn print_as_table(json_value: &Value, capture_output: bool) -> Result<Option<String>, Error> {
    match json_value {
        Value::Array(items) => {
            if items.is_empty() {
                return Ok(output_or_capture(constants::EMPTY_ARRAY, capture_output));
            }

            if items.len() > MAX_TABLE_ROWS {
                let msg = format!(
                    "Array too large: {} items (max {} for table display)\nUse --format json or --jq to process the full data",
                    items.len(),
                    MAX_TABLE_ROWS
                );
                return Ok(output_or_capture(&msg, capture_output));
            }

            let Some(Value::Object(_)) = items.first() else {
                return Ok(print_numbered_list(items, capture_output));
            };

            let mut table_data: Vec<BTreeMap<String, String>> = Vec::new();

            for item in items {
                let Value::Object(obj) = item else {
                    continue;
                };
                let mut row = BTreeMap::new();
                for (key, value) in obj {
                    row.insert(key.clone(), format_value_for_table(value));
                }
                table_data.push(row);
            }

            if table_data.is_empty() {
                return Ok(print_numbered_list(items, capture_output));
            }

            let mut rows = Vec::new();
            for (i, row) in table_data.iter().enumerate() {
                if i > 0 {
                    rows.push(TableRow {
                        key: "---".to_string(),
                        value: "---".to_string(),
                    });
                }
                for (key, value) in row {
                    rows.push(TableRow {
                        key: key.clone(),
                        value: value.clone(),
                    });
                }
            }

            let table = Table::new(&rows);
            Ok(output_or_capture(&table.to_string(), capture_output))
        }
        Value::Object(obj) => {
            if obj.len() > MAX_TABLE_ROWS {
                let msg = format!(
                    "Object too large: {} fields (max {} for table display)\nUse --format json or --jq to process the full data",
                    obj.len(),
                    MAX_TABLE_ROWS
                );
                return Ok(output_or_capture(&msg, capture_output));
            }

            let rows: Vec<KeyValue> = obj
                .iter()
                .map(|(key, value)| KeyValue {
                    key: key.clone(),
                    value: format_value_for_table(value),
                })
                .collect();

            let table = Table::new(&rows);
            Ok(output_or_capture(&table.to_string(), capture_output))
        }
        _ => {
            let formatted = format_value_for_table(json_value);
            Ok(output_or_capture(&formatted, capture_output))
        }
    }
}

/// Formats a JSON value for display in a table cell.
fn format_value_for_table(value: &Value) -> String {
    match value {
        Value::Null => constants::NULL_VALUE.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            if arr.len() <= 3 {
                format!(
                    "[{}]",
                    arr.iter()
                        .map(format_value_for_table)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("[{} items]", arr.len())
            }
        }
        Value::Object(obj) => {
            if obj.len() <= 2 {
                format!(
                    "{{{}}}",
                    obj.iter()
                        .map(|(k, v)| format!("{}: {}", k, format_value_for_table(v)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                format!("{{object with {} fields}}", obj.len())
            }
        }
    }
}
