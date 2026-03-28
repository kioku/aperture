//! Automatic pagination loop for `--auto-paginate`.
//!
//! Calls [`executor::execute`] repeatedly until the last page is reached,
//! printing each item as a line of NDJSON to stdout.
//!
//! # Strategies
//!
//! | Strategy | How "next page" is determined |
//! |---|---|
//! | Cursor | A field in the response body carries the next-page token; injected as a query param. |
//! | Offset | The `page` or `offset` query param is incremented by the returned page size. |
//! | LinkHeader | The RFC 5988 `Link: <url>; rel="next"` response header provides the next URL. |
//! | None | Warning is printed and the operation runs once (no loop). |

use crate::cache::models::{CachedSpec, PaginationStrategy};
use crate::constants;
use crate::engine::executor;
use crate::error::Error;
use crate::invocation::{ExecutionContext, ExecutionResult, OperationCall};
use serde_json::Value;
use std::collections::HashMap;

/// Hard page cap: prevents runaway loops on pathological or misconfigured APIs.
const MAX_PAGES: usize = 1000;

/// Response body keys searched (in order) to locate the data array when the
/// top-level response is an object rather than a bare array.
const DATA_ARRAY_FIELDS: &[&str] = &["data", "items", "results", "entries", "records", "content"];

// ── Public entry point ────────────────────────────────────────────────────

struct PagePayload {
    body: String,
    response_headers: HashMap<String, String>,
}

fn write_json_line<W: std::io::Write + ?Sized, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), Error> {
    let line = serde_json::to_string(value)
        .map_err(|e| Error::serialization_error(format!("Failed to serialize output line: {e}")))?;
    writeln!(writer, "{line}").map_err(|e| Error::io_error(format!("Failed to write output: {e}")))
}

async fn fetch_page_payload<W: std::io::Write + ?Sized>(
    spec: &CachedSpec,
    call: OperationCall,
    ctx: ExecutionContext,
    writer: &mut W,
) -> Result<Option<PagePayload>, Error> {
    let result = executor::execute(spec, call, ctx).await?;

    match result {
        ExecutionResult::Success { body, headers, .. } => Ok(Some(PagePayload {
            body,
            response_headers: headers,
        })),
        ExecutionResult::Cached { body } => Ok(Some(PagePayload {
            body,
            response_headers: HashMap::new(),
        })),
        ExecutionResult::DryRun { request_info } => {
            write_json_line(writer, &request_info)?;
            Ok(None)
        }
        ExecutionResult::Empty => Ok(None),
    }
}

fn emit_items<W: std::io::Write + ?Sized>(json: &Value, writer: &mut W) -> Result<usize, Error> {
    let items = extract_items(json);
    let page_len = items.len();

    for item in items {
        write_json_line(writer, item)?;
    }

    Ok(page_len)
}

/// Runs the pagination loop, writing each result item as a NDJSON line to
/// `writer`.
///
/// Returns the total number of items emitted across all pages.
///
/// # Errors
///
/// Returns an error on HTTP failure or malformed JSON. A partial result may
/// already have been written to `writer` before the error occurs.
#[allow(clippy::too_many_lines)]
pub async fn execute_paginated(
    spec: &CachedSpec,
    mut call: OperationCall,
    ctx: ExecutionContext,
    writer: &mut impl std::io::Write,
) -> Result<u64, Error> {
    let operation = spec
        .commands
        .iter()
        .find(|c| c.operation_id == call.operation_id)
        .ok_or_else(|| Error::operation_not_found(&call.operation_id))?;

    let strategy = operation.pagination.strategy;

    if matches!(strategy, PaginationStrategy::None) {
        tracing::warn!(
            operation_id = %call.operation_id,
            "No pagination metadata detected for this operation; executing once. \
             Consider adding x-aperture-pagination to the spec."
        );
    }

    let cursor_field = operation.pagination.cursor_field.clone();
    let cursor_param = operation
        .pagination
        .cursor_param
        .clone()
        .or_else(|| cursor_field.clone());

    let page_param = operation
        .pagination
        .page_param
        .clone()
        .unwrap_or_else(|| detect_page_param(&call.query_params));

    let limit_param = operation
        .pagination
        .limit_param
        .clone()
        .unwrap_or_else(|| detect_limit_param(&call.query_params));

    let limit: usize = call
        .query_params
        .get(&limit_param)
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    let mut total_items: u64 = 0;

    for _page_num in 0..MAX_PAGES {
        let Some(PagePayload {
            body,
            response_headers,
        }) = fetch_page_payload(spec, call.clone(), ctx.clone(), writer).await?
        else {
            break;
        };

        let json: Value = serde_json::from_str(&body).map_err(|e| {
            Error::invalid_json_body(format!("Page response is not valid JSON: {e}"))
        })?;

        let page_len = emit_items(&json, writer)?;
        total_items += page_len as u64;

        // Determine next page coordinates; break if this was the last one.
        let has_next = advance_cursor(
            strategy,
            &mut call,
            &json,
            &response_headers,
            cursor_field.as_ref(),
            cursor_param.as_ref(),
            &page_param,
            page_len,
            limit,
        );
        if !has_next {
            break;
        }
    }

    Ok(total_items)
}

// ── Pagination advance helpers ────────────────────────────────────────────

/// Mutates `call.query_params` to point to the next page and returns `true`
/// if there is a next page. Returns `false` when the caller should stop.
#[allow(clippy::too_many_arguments)]
fn advance_cursor(
    strategy: PaginationStrategy,
    call: &mut OperationCall,
    json: &Value,
    response_headers: &HashMap<String, String>,
    cursor_field: Option<&String>,
    cursor_param: Option<&String>,
    page_param: &str,
    page_len: usize,
    limit: usize,
) -> bool {
    match strategy {
        PaginationStrategy::None => false,

        PaginationStrategy::Cursor => {
            advance_cursor_strategy(call, json, cursor_field, cursor_param)
        }

        PaginationStrategy::Offset => advance_offset_strategy(call, page_param, page_len, limit),

        PaginationStrategy::LinkHeader => advance_link_header_strategy(call, response_headers),
    }
}

/// Advances cursor-based pagination. Returns `true` if a non-empty cursor was
/// found and set.
fn advance_cursor_strategy(
    call: &mut OperationCall,
    json: &Value,
    cursor_field: Option<&String>,
    cursor_param: Option<&String>,
) -> bool {
    let field = cursor_field.map_or("next_cursor", String::as_str);
    let param = cursor_param.map_or(field, String::as_str);
    match extract_cursor_value(json, field) {
        Some(c) if !c.is_empty() => {
            call.query_params.insert(param.to_string(), c);
            true
        }
        _ => false,
    }
}

/// Advances offset/page-number pagination. Returns `true` if the page was
/// full (i.e., there may be more data).
///
/// `"offset"` and `"skip"` are zero-based record counts (advance by
/// `page_len`); everything else (e.g. `"page"`) is a 1-based page number.
fn advance_offset_strategy(
    call: &mut OperationCall,
    page_param: &str,
    page_len: usize,
    limit: usize,
) -> bool {
    if page_len == 0 || page_len < limit {
        return false;
    }

    let is_record_offset = page_param == "offset" || page_param == "skip";
    let next_value = if is_record_offset {
        let current: usize = call
            .query_params
            .get(page_param)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        current + page_len
    } else {
        let current: usize = call
            .query_params
            .get(page_param)
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        current + 1
    };

    call.query_params
        .insert(page_param.to_string(), next_value.to_string());
    true
}

/// Advances Link-header pagination. Returns `true` if a `rel="next"` URL was
/// found and applied.
fn advance_link_header_strategy(
    call: &mut OperationCall,
    response_headers: &HashMap<String, String>,
) -> bool {
    let link_value = response_headers
        .iter()
        .find(|(k, _)| k.to_lowercase() == constants::HEADER_LINK)
        .map_or("", |(_, v)| v.as_str());

    parse_link_next(link_value).is_some_and(|next_url| apply_next_url(call, &next_url))
}

// ── Item extraction ──────────────────────────────────────────────────────

/// Extracts the items list from a paginated response.
///
/// Tries the response root first (if it's an array), then looks for
/// well-known wrapper field names.
fn extract_items(json: &Value) -> Vec<&Value> {
    match json {
        Value::Array(arr) => arr.iter().collect(),
        Value::Object(_) => {
            for field in DATA_ARRAY_FIELDS {
                if let Some(Value::Array(arr)) = json.get(*field) {
                    return arr.iter().collect();
                }
            }
            // Fallback: treat the whole object as a single item.
            std::slice::from_ref(json).iter().collect()
        }
        _ => vec![],
    }
}

// ── Cursor extraction ────────────────────────────────────────────────────

/// Extracts a string cursor value from a JSON response body.
///
/// Supports dotted paths (e.g. `"page.next_cursor"`).
fn extract_cursor_value(json: &Value, field: &str) -> Option<String> {
    let mut current = json;
    for part in field.split('.') {
        current = current.get(part)?;
    }
    match current {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

// ── Link header parsing ───────────────────────────────────────────────────

/// Parses an RFC 5988 `Link` header value and returns the `rel="next"` URL.
///
/// Example input: `<https://api.example.com/items?page=2>; rel="next",
///                 <https://api.example.com/items?page=10>; rel="last"`
#[must_use]
pub fn parse_link_next(header_value: &str) -> Option<String> {
    for part in header_value.split(',') {
        let part = part.trim();
        let Some(url_end) = part.find('>') else {
            continue;
        };
        if !part.starts_with('<') {
            continue;
        }
        let url = &part[1..url_end];
        let rest = &part[url_end + 1..];
        if rest.split(';').any(|seg| {
            let seg = seg.trim().to_lowercase();
            seg == r#"rel="next""# || seg == "rel=next"
        }) {
            return Some(url.to_string());
        }
    }
    None
}

// ── URL application for LinkHeader strategy ──────────────────────────────

/// Updates `call.query_params` from the query string of a fully-qualified next
/// URL, keeping the rest of the call unchanged.
///
/// Returns `true` if the call was successfully updated with new parameters,
/// `false` if the URL had no usable query string (caller should stop paginating).
fn apply_next_url(call: &mut OperationCall, next_url: &str) -> bool {
    let query_str = if let Some(pos) = next_url.find('?') {
        &next_url[pos + 1..]
    } else {
        tracing::warn!(
            next_url,
            "Link next URL has no query string; stopping pagination"
        );
        return false;
    };

    let new_params: HashMap<String, String> = query_str
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            let key = parts.next().filter(|k| !k.is_empty())?;
            let val = parts.next().unwrap_or("");
            Some((
                urlencoding::decode(key).unwrap_or_default().into_owned(),
                urlencoding::decode(val).unwrap_or_default().into_owned(),
            ))
        })
        .collect();

    if new_params.is_empty() {
        return false;
    }

    call.query_params = new_params;
    true
}

// ── Parameter detection heuristics ───────────────────────────────────────

/// Returns the first `page`/`offset`/`skip` query param present in `params`,
/// or `"page"` as a default.
fn detect_page_param(params: &HashMap<String, String>) -> String {
    constants::PAGINATION_PAGE_PARAMS
        .iter()
        .find(|&&p| params.contains_key(p))
        .map_or("page", |&p| p)
        .to_string()
}

/// Returns the first `limit`/`per_page`/`page_size` query param present in
/// `params`, or `"limit"` as a default.
fn detect_limit_param(params: &HashMap<String, String>) -> String {
    constants::PAGINATION_LIMIT_PARAMS
        .iter()
        .find(|&&p| params.contains_key(p))
        .map_or("limit", |&p| p)
        .to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_link_next ───────────────────────────────────────────────────

    #[test]
    fn test_parse_link_next_returns_next_url() {
        let header = r#"<https://api.example.com/items?page=2>; rel="next", <https://api.example.com/items?page=10>; rel="last""#;
        assert_eq!(
            parse_link_next(header),
            Some("https://api.example.com/items?page=2".to_string())
        );
    }

    #[test]
    fn test_parse_link_next_without_next_returns_none() {
        let header = r#"<https://api.example.com/items?page=10>; rel="last""#;
        assert_eq!(parse_link_next(header), None);
    }

    #[test]
    fn test_parse_link_next_without_quotes() {
        let header = "<https://api.example.com/items?page=2>; rel=next";
        assert_eq!(
            parse_link_next(header),
            Some("https://api.example.com/items?page=2".to_string())
        );
    }

    #[test]
    fn test_parse_link_next_empty_returns_none() {
        assert_eq!(parse_link_next(""), None);
    }

    // ── extract_cursor_value ──────────────────────────────────────────────

    #[test]
    fn test_extract_cursor_value_simple_field() {
        let json = serde_json::json!({"next_cursor": "abc123", "data": []});
        assert_eq!(
            extract_cursor_value(&json, "next_cursor"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_extract_cursor_value_dotted_path() {
        let json = serde_json::json!({"page": {"next_cursor": "tok_xyz"}});
        assert_eq!(
            extract_cursor_value(&json, "page.next_cursor"),
            Some("tok_xyz".to_string())
        );
    }

    #[test]
    fn test_extract_cursor_value_null_returns_none() {
        let json = serde_json::json!({"next_cursor": null});
        assert_eq!(extract_cursor_value(&json, "next_cursor"), None);
    }

    #[test]
    fn test_extract_cursor_value_empty_string_returns_none() {
        let json = serde_json::json!({"next_cursor": ""});
        // Empty string means no cursor — callers treat it as termination.
        assert_eq!(extract_cursor_value(&json, "next_cursor"), None);
    }

    // ── extract_items ─────────────────────────────────────────────────────

    #[test]
    fn test_extract_items_from_top_level_array() {
        let json = serde_json::json!([{"id": 1}, {"id": 2}]);
        assert_eq!(extract_items(&json).len(), 2);
    }

    #[test]
    fn test_extract_items_from_data_wrapper() {
        let json = serde_json::json!({"data": [{"id": 1}], "total": 1});
        assert_eq!(extract_items(&json).len(), 1);
    }

    #[test]
    fn test_extract_items_from_items_wrapper() {
        let json = serde_json::json!({"items": [{"id": 1}, {"id": 2}], "next_cursor": "abc"});
        assert_eq!(extract_items(&json).len(), 2);
    }

    #[test]
    fn test_extract_items_single_object_fallback() {
        let json = serde_json::json!({"id": 1, "name": "Alice"});
        // No array wrapper — treated as a single item.
        assert_eq!(extract_items(&json).len(), 1);
    }

    #[test]
    fn test_extract_items_empty_array() {
        let json = serde_json::json!([]);
        assert_eq!(extract_items(&json).len(), 0);
    }

    // ── advance_offset_strategy ───────────────────────────────────────────

    #[test]
    fn test_advance_offset_strategy_increments_page_number() {
        let mut call = crate::invocation::OperationCall {
            operation_id: "op".to_string(),
            path_params: HashMap::new(),
            query_params: HashMap::from([("page".to_string(), "1".to_string())]),
            header_params: HashMap::new(),
            body: None,
            custom_headers: vec![],
        };
        let has_next = advance_offset_strategy(&mut call, "page", 10, 10);
        assert!(has_next);
        assert_eq!(call.query_params["page"], "2");
    }

    #[test]
    fn test_advance_offset_strategy_stops_on_partial_page() {
        let mut call = crate::invocation::OperationCall {
            operation_id: "op".to_string(),
            path_params: HashMap::new(),
            query_params: HashMap::from([("page".to_string(), "1".to_string())]),
            header_params: HashMap::new(),
            body: None,
            custom_headers: vec![],
        };
        let has_next = advance_offset_strategy(&mut call, "page", 3, 10);
        assert!(!has_next, "partial page should return false");
    }

    #[test]
    fn test_advance_offset_strategy_skip_advances_by_page_len() {
        let mut call = crate::invocation::OperationCall {
            operation_id: "op".to_string(),
            path_params: HashMap::new(),
            query_params: HashMap::from([("skip".to_string(), "0".to_string())]),
            header_params: HashMap::new(),
            body: None,
            custom_headers: vec![],
        };
        let has_next = advance_offset_strategy(&mut call, "skip", 10, 10);
        assert!(has_next);
        assert_eq!(call.query_params["skip"], "10");

        let has_next = advance_offset_strategy(&mut call, "skip", 10, 10);
        assert!(has_next);
        assert_eq!(call.query_params["skip"], "20");
    }

    #[test]
    fn test_advance_offset_strategy_offset_advances_by_page_len() {
        let mut call = crate::invocation::OperationCall {
            operation_id: "op".to_string(),
            path_params: HashMap::new(),
            query_params: HashMap::from([("offset".to_string(), "0".to_string())]),
            header_params: HashMap::new(),
            body: None,
            custom_headers: vec![],
        };
        let has_next = advance_offset_strategy(&mut call, "offset", 5, 5);
        assert!(has_next);
        assert_eq!(call.query_params["offset"], "5");
    }
}
