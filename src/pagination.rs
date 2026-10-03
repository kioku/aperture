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
use std::collections::{HashMap, HashSet};

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

struct PaginationState {
    strategy: PaginationStrategy,
    cursor_field: Option<String>,
    cursor_param: Option<String>,
    page_param: String,
    limit: usize,
}

fn write_json_line<W: std::io::Write + ?Sized, T: serde::Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<bool, Error> {
    let line = serde_json::to_string(value)
        .map_err(|e| Error::serialization_error(format!("Failed to serialize output line: {e}")))?;
    // Flush per item so buffered writers expose a closed consumer before the
    // next page is fetched.
    match writeln!(writer, "{line}").and_then(|()| writer.flush()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(Error::io_error(format!("Failed to write output: {e}"))),
    }
}

async fn fetch_page_payload<W: std::io::Write + ?Sized>(
    spec: &CachedSpec,
    call: OperationCall,
    ctx: ExecutionContext,
    writer: &mut W,
) -> Result<Option<PagePayload>, Error> {
    let result = executor::execute(spec, call, ctx).await?;

    match result {
        ExecutionResult::Success { body, headers, .. }
        | ExecutionResult::Cached { body, headers, .. } => Ok(Some(PagePayload {
            body,
            response_headers: headers,
        })),
        ExecutionResult::DryRun { request_info } => {
            write_json_line(writer, &request_info)?;
            Ok(None)
        }
        ExecutionResult::Binary { .. } => Err(Error::validation_error(
            "Binary responses cannot be auto-paginated",
        )),
        ExecutionResult::Empty => Ok(None),
    }
}

fn emit_items<W: std::io::Write + ?Sized>(
    json: &Value,
    writer: &mut W,
) -> Result<(usize, bool), Error> {
    let items = extract_items(json);
    for (emitted, item) in items.iter().enumerate() {
        if !write_json_line(writer, item)? {
            return Ok((emitted, false));
        }
    }
    Ok((items.len(), true))
}

fn resolve_pagination_state(
    operation: &crate::cache::models::CachedCommand,
    call: &OperationCall,
) -> PaginationState {
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

    if matches!(operation.pagination.strategy, PaginationStrategy::None) {
        tracing::warn!(
            operation_id = %call.operation_id,
            "No pagination metadata detected for this operation; executing once. \
             Consider adding x-aperture-pagination to the spec."
        );
    }

    PaginationState {
        strategy: operation.pagination.strategy,
        cursor_field,
        cursor_param,
        page_param,
        limit,
    }
}

async fn process_paginated_page<W: std::io::Write + ?Sized>(
    spec: &CachedSpec,
    call: &mut OperationCall,
    ctx: ExecutionContext,
    writer: &mut W,
    state: &PaginationState,
) -> Result<Option<(usize, bool)>, Error> {
    let Some(PagePayload {
        body,
        response_headers,
    }) = fetch_page_payload(spec, call.clone(), ctx.clone(), writer).await?
    else {
        return Ok(None);
    };

    let json: Value = serde_json::from_str(&body)
        .map_err(|e| Error::invalid_json_body(format!("Page response is not valid JSON: {e}")))?;

    let (page_len, output_open) = emit_items(&json, writer)?;
    if !output_open {
        return Ok(Some((page_len, false)));
    }
    if matches!(state.strategy, PaginationStrategy::LinkHeader) {
        let current = executor::pagination_request_url(spec, call, &ctx)?;
        let has_next = advance_link_url(call, &response_headers, &current)?;
        return Ok(Some((page_len, has_next)));
    }
    let has_next = advance_cursor(
        state.strategy,
        call,
        &json,
        state.cursor_field.as_ref(),
        state.cursor_param.as_ref(),
        &state.page_param,
        page_len,
        state.limit,
    );

    Ok(Some((page_len, has_next)))
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
pub async fn execute_paginated(
    spec: &CachedSpec,
    mut call: OperationCall,
    mut ctx: ExecutionContext,
    writer: &mut impl std::io::Write,
) -> Result<u64, Error> {
    let operation = spec
        .commands
        .iter()
        .find(|c| c.operation_id == call.operation_id)
        .ok_or_else(|| Error::operation_not_found(&call.operation_id))?;

    ctx.auto_paginate = true;
    let state = resolve_pagination_state(operation, &call);

    let mut total_items: u64 = 0;

    let mut visited = HashSet::new();
    for page_num in 0..MAX_PAGES {
        record_page(spec, &call, &ctx, &mut visited)?;
        let Some((page_len, has_next)) =
            process_paginated_page(spec, &mut call, ctx.clone(), writer, &state).await?
        else {
            break;
        };

        total_items += page_len as u64;

        if !has_next {
            break;
        }
        check_page_cap(page_num + 1)?;
    }

    Ok(total_items)
}

fn check_page_cap(pages: usize) -> Result<(), Error> {
    if pages == MAX_PAGES {
        return Err(Error::validation_error(
            "Pagination page cap reached with more data pending; results are incomplete",
        ));
    }
    Ok(())
}

fn record_page(
    spec: &CachedSpec,
    call: &OperationCall,
    ctx: &ExecutionContext,
    visited: &mut HashSet<String>,
) -> Result<(), Error> {
    let url = executor::pagination_request_url(spec, call, ctx)?;
    if !visited.insert(url.to_string()) {
        return Err(Error::validation_error(
            "Pagination loop detected; results are incomplete",
        ));
    }
    Ok(())
}

// ── Pagination advance helpers ────────────────────────────────────────────

/// Mutates `call.query_params` to point to the next page and returns `true`
/// if there is a next page. Returns `false` when the caller should stop.
#[allow(clippy::too_many_arguments)]
fn advance_cursor(
    strategy: PaginationStrategy,
    call: &mut OperationCall,
    json: &Value,
    cursor_field: Option<&String>,
    cursor_param: Option<&String>,
    page_param: &str,
    page_len: usize,
    limit: usize,
) -> bool {
    match strategy {
        PaginationStrategy::None | PaginationStrategy::LinkHeader => false,

        PaginationStrategy::Cursor => {
            advance_cursor_strategy(call, json, cursor_field, cursor_param)
        }

        PaginationStrategy::Offset => advance_offset_strategy(call, page_param, page_len, limit),
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

/// Relative links resolve against the current page. Authentication remains attached
/// only to same-origin targets; the executor repeats this check before sending.
fn advance_link_url(
    call: &mut OperationCall,
    headers: &HashMap<String, String>,
    current: &reqwest::Url,
) -> Result<bool, Error> {
    let link = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(constants::HEADER_LINK))
        .map_or("", |(_, value)| value.as_str());
    let Some(next) = checked_link_next(link)? else {
        return Ok(false);
    };
    let target = current
        .join(&next)
        .map_err(|e| Error::validation_error(format!("Invalid pagination next URL: {e}")))?;
    executor::validate_pagination_target(current, &target)?;
    call.pagination_url = Some(target);
    Ok(true)
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
    checked_link_next(header_value).ok().flatten()
}

/// Delimiters in URI references and quoted parameter values are data, not
/// link separators. Reject malformed syntax rather than reporting completion.
#[derive(Default)]
struct LinkSyntax {
    quoted: bool,
    escaped: bool,
    in_uri: bool,
}

impl LinkSyntax {
    const fn consume_quoted(&mut self, character: char) {
        match character {
            '\\' => self.escaped = true,
            '"' => self.quoted = false,
            _ => {}
        }
    }

    const fn consume(&mut self, character: char) -> bool {
        if self.escaped {
            self.escaped = false;
            return false;
        }
        if self.quoted {
            self.consume_quoted(character);
            return false;
        }
        if self.in_uri {
            self.in_uri = character != '>';
            return false;
        }
        match character {
            '<' => self.in_uri = true,
            '"' => self.quoted = true,
            _ => return true,
        }
        false
    }
}

fn link_parts(value: &str, separator: char) -> Result<Vec<&str>, Error> {
    let mut syntax = LinkSyntax::default();
    let mut start = 0;
    let mut parts = Vec::new();
    for (offset, character) in value.char_indices() {
        if syntax.consume(character) && character == separator {
            parts.push(value[start..offset].trim());
            start = offset + character.len_utf8();
        }
    }
    if syntax.quoted || syntax.in_uri {
        return Err(Error::validation_error("Malformed pagination Link header"));
    }
    parts.push(value[start..].trim());
    Ok(parts)
}

fn link_relation_value(value: &str) -> Result<&str, Error> {
    let relation = if let Some(quoted) = value.strip_prefix('"') {
        quoted
            .strip_suffix('"')
            .ok_or_else(|| Error::validation_error("Malformed pagination Link relation"))?
    } else {
        if value.bytes().any(|byte| byte.is_ascii_whitespace()) {
            return Err(Error::validation_error(
                "Malformed pagination Link relation",
            ));
        }
        value
    };
    if relation.is_empty() || relation.contains(['"', '\\']) {
        return Err(Error::validation_error(
            "Malformed pagination Link relation",
        ));
    }
    Ok(relation)
}

fn has_next_relation(parameters: &str) -> Result<bool, Error> {
    let mut relation = None;
    for parameter in link_parts(parameters, ';')? {
        let Some((name, value)) = parameter.split_once('=') else {
            if parameter.trim().eq_ignore_ascii_case("rel") {
                return Err(Error::validation_error(
                    "Malformed pagination Link relation",
                ));
            }
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("rel") {
            continue;
        }
        if relation.is_some() {
            return Err(Error::validation_error(
                "Ambiguous pagination Link relation",
            ));
        }
        relation = Some(link_relation_value(value.trim())?);
    }
    Ok(relation.is_some_and(|value| {
        value
            .split_ascii_whitespace()
            .any(|token| token.eq_ignore_ascii_case("next"))
    }))
}

fn validate_link_part(uri: &str, parameters: &str) -> Result<(), Error> {
    let parameters = parameters.trim();
    if uri.is_empty() || (!parameters.is_empty() && !parameters.starts_with(';')) {
        return Err(Error::validation_error("Malformed pagination Link header"));
    }
    Ok(())
}

fn next_link_part(part: &str) -> Result<Option<&str>, Error> {
    let (uri, parameters) = part
        .strip_prefix('<')
        .and_then(|part| part.split_once('>'))
        .ok_or_else(|| Error::validation_error("Malformed pagination Link header"))?;
    validate_link_part(uri, parameters)?;
    if has_next_relation(parameters)? {
        Ok(Some(uri))
    } else {
        Ok(None)
    }
}

fn checked_link_next(header: &str) -> Result<Option<String>, Error> {
    if header.trim().is_empty() {
        return Ok(None);
    }
    let mut next = None;
    for part in link_parts(header, ',')? {
        let Some(uri) = next_link_part(part)? else {
            continue;
        };
        if next.is_some() {
            return Err(Error::validation_error("Ambiguous pagination next links"));
        }
        next = Some(uri.to_string());
    }
    Ok(next)
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

    struct BrokenPipeWriter;

    impl std::io::Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed pipe",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    struct OtherErrorWriter;

    impl std::io::Write for OtherErrorWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_write_json_line_ignores_broken_pipe() {
        let mut writer = BrokenPipeWriter;
        assert!(!write_json_line(&mut writer, &serde_json::json!({"id": 1})).unwrap());
    }

    #[test]
    fn test_write_json_line_surfaces_other_errors() {
        let mut writer = OtherErrorWriter;
        assert!(write_json_line(&mut writer, &serde_json::json!({"id": 1})).is_err());
    }

    // ── advance_offset_strategy ───────────────────────────────────────────

    #[test]
    fn test_advance_offset_strategy_increments_page_number() {
        let mut call = crate::invocation::OperationCall {
            pagination_url: None,
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
            pagination_url: None,
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
            pagination_url: None,
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
            pagination_url: None,
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

#[cfg(test)]
mod link_adversarial_tests {
    use super::checked_link_next;

    #[test]
    fn link_syntax_sweep() {
        for header in ["", "  ", "</last>; rel=last", "</a>; title=unknown"] {
            assert_eq!(checked_link_next(header).unwrap(), None);
        }
        for header in [
            "garbage",
            "<>; rel=next",
            "</a>rel=next",
            "</a>; rel",
            "</a>; rel=",
            "</a>; rel=prev next",
            "</a>; rel=\"next\"junk",
            "</a",
            "</a>; rel=\"next",
            "</a>; rel=next; rel=last",
            "</a>; rel=next, </a>; rel=next",
        ] {
            assert!(checked_link_next(header).is_err(), "{header}");
        }
        assert_eq!(
            checked_link_next("</a>; rel=NEXT").unwrap(),
            Some("/a".into())
        );
        assert_eq!(
            checked_link_next(r#"</a?tag=x,y>; title="semi;comma,quote\""; rel="prev next""#)
                .unwrap(),
            Some("/a?tag=x,y".into())
        );
    }
}
