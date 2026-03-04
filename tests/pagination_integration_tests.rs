//! Integration tests for `--auto-paginate` (cursor, offset, and Link-header strategies).
//!
//! Each test stands up a `wiremock` mock server that simulates a multi-page API
//! and calls [`execute_paginated`] directly, capturing NDJSON written to a
//! `Vec<u8>` buffer instead of stdout.

mod test_helpers;

use aperture_cli::cache::models::{CachedSpec, PaginationInfo, PaginationStrategy};
use aperture_cli::invocation::{ExecutionContext, OperationCall};
use aperture_cli::pagination::execute_paginated;
use std::collections::HashMap;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ── Helpers ───────────────────────────────────────────────────────────────

fn make_spec_with_pagination(base_url: &str, pagination: PaginationInfo) -> CachedSpec {
    let cmd = aperture_cli::cache::models::CachedCommand {
        pagination,
        ..test_helpers::test_command("items", "listItems", "GET", "/items")
    };
    CachedSpec {
        cache_format_version: aperture_cli::cache::models::CACHE_FORMAT_VERSION,
        name: "test-api".to_string(),
        version: "1.0.0".to_string(),
        commands: vec![cmd],
        base_url: Some(base_url.to_string()),
        servers: vec![base_url.to_string()],
        security_schemes: HashMap::new(),
        skipped_endpoints: vec![],
        server_variables: HashMap::new(),
    }
}

const fn base_ctx() -> ExecutionContext {
    ExecutionContext {
        dry_run: false,
        idempotency_key: None,
        cache_config: None,
        retry_context: None,
        base_url: None,
        global_config: None,
        server_var_args: vec![],
        auto_paginate: true,
    }
}

fn base_call(query_params: HashMap<String, String>) -> OperationCall {
    OperationCall {
        operation_id: "listItems".to_string(),
        path_params: HashMap::new(),
        query_params,
        header_params: HashMap::new(),
        body: None,
        custom_headers: vec![],
    }
}

/// Parses NDJSON from a buffer into a `Vec<serde_json::Value>`.
fn parse_ndjson(buf: &[u8]) -> Vec<serde_json::Value> {
    let text = std::str::from_utf8(buf).expect("output should be valid UTF-8");
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("each line should be valid JSON"))
        .collect()
}

// ── Cursor pagination ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_cursor_pagination_collects_all_pages() {
    let server = MockServer::start().await;

    // Page 1: returns 2 items + next cursor
    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": 1}, {"id": 2}],
            "next_cursor": "cursor_page2"
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Page 2: returns 2 items + next cursor
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("cursor", "cursor_page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": 3}, {"id": 4}],
            "next_cursor": "cursor_page3"
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Page 3: last page — null cursor
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("cursor", "cursor_page3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": 5}],
            "next_cursor": null
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let spec = make_spec_with_pagination(
        &server.uri(),
        PaginationInfo {
            strategy: PaginationStrategy::Cursor,
            cursor_field: Some("next_cursor".to_string()),
            cursor_param: Some("cursor".to_string()),
            page_param: None,
            limit_param: None,
        },
    );

    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(HashMap::new()), base_ctx(), &mut buf)
        .await
        .expect("execute_paginated should succeed");

    assert_eq!(count, 5, "should have collected 5 items across 3 pages");

    let items = parse_ndjson(&buf);
    assert_eq!(items.len(), 5);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[4]["id"], 5);
}

#[tokio::test]
async fn test_cursor_pagination_stops_on_empty_cursor() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": 1}],
            "next_cursor": ""  // empty string = done
        })))
        .mount(&server)
        .await;

    let spec = make_spec_with_pagination(
        &server.uri(),
        PaginationInfo {
            strategy: PaginationStrategy::Cursor,
            cursor_field: Some("next_cursor".to_string()),
            cursor_param: Some("cursor".to_string()),
            page_param: None,
            limit_param: None,
        },
    );

    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(HashMap::new()), base_ctx(), &mut buf)
        .await
        .expect("should succeed");

    assert_eq!(count, 1);
}

// ── Offset / page-number pagination ──────────────────────────────────────

#[tokio::test]
async fn test_offset_pagination_page_style_collects_all_pages() {
    let server = MockServer::start().await;

    // Page 1 (implicit: no page param or page=1)
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("limit", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id": 1}, {"id": 2}])),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Page 2
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("page", "2"))
        .and(query_param("limit", "2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id": 3}])), // partial page = last
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let spec = make_spec_with_pagination(
        &server.uri(),
        PaginationInfo {
            strategy: PaginationStrategy::Offset,
            cursor_field: None,
            cursor_param: None,
            page_param: Some("page".to_string()),
            limit_param: Some("limit".to_string()),
        },
    );

    let mut params = HashMap::new();
    params.insert("limit".to_string(), "2".to_string());

    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(params), base_ctx(), &mut buf)
        .await
        .expect("should succeed");

    assert_eq!(count, 3, "should collect items from both pages");
    let items = parse_ndjson(&buf);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[2]["id"], 3);
}

#[tokio::test]
async fn test_offset_pagination_stops_on_empty_page() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([])), // empty
        )
        .mount(&server)
        .await;

    let spec = make_spec_with_pagination(
        &server.uri(),
        PaginationInfo {
            strategy: PaginationStrategy::Offset,
            cursor_field: None,
            cursor_param: None,
            page_param: Some("page".to_string()),
            limit_param: Some("limit".to_string()),
        },
    );

    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(HashMap::new()), base_ctx(), &mut buf)
        .await
        .expect("should succeed");

    assert_eq!(count, 0);
    assert!(buf.is_empty());
}

// ── Link-header pagination ────────────────────────────────────────────────

#[tokio::test]
async fn test_link_header_pagination_collects_all_pages() {
    let server = MockServer::start().await;
    let base = server.uri();
    let page2_url = format!("{base}/items?page=2");
    let link_header = format!(r#"<{page2_url}>; rel="next", <{base}/items?page=5>; rel="last""#);

    // Page 1: responds with Link header pointing to page 2
    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", link_header.as_str())
                .set_body_json(serde_json::json!([{"id": 1}, {"id": 2}])),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Page 2: no Link header — last page
    Mock::given(method("GET"))
        .and(path("/items"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id": 3}])))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let spec = make_spec_with_pagination(
        &server.uri(),
        PaginationInfo {
            strategy: PaginationStrategy::LinkHeader,
            cursor_field: None,
            cursor_param: None,
            page_param: None,
            limit_param: None,
        },
    );

    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(HashMap::new()), base_ctx(), &mut buf)
        .await
        .expect("should succeed");

    assert_eq!(count, 3, "should collect 3 items across 2 pages");
    let items = parse_ndjson(&buf);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[2]["id"], 3);
}

// ── No-strategy fallback ─────────────────────────────────────────────────

#[tokio::test]
async fn test_no_strategy_runs_once() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/items"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id": 1}, {"id": 2}])),
        )
        .mount(&server)
        .await;

    let spec = make_spec_with_pagination(
        &server.uri(),
        PaginationInfo {
            strategy: PaginationStrategy::None,
            cursor_field: None,
            cursor_param: None,
            page_param: None,
            limit_param: None,
        },
    );

    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(HashMap::new()), base_ctx(), &mut buf)
        .await
        .expect("should succeed");

    assert_eq!(count, 2, "should have output 2 items from the single page");

    // Only one request should have been made
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}
