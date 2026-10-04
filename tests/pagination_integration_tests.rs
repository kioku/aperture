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

fn base_ctx() -> ExecutionContext {
    ExecutionContext {
        http_clients: aperture_cli::engine::executor::HttpClientPool::default(),
        dry_run: false,
        idempotency_key: None,
        cache_config: None,
        retry_context: None,
        base_url: None,
        proxy_override: aperture_cli::invocation::ProxyOverride::Default,
        global_config: None,
        server_var_args: vec![],
        auto_paginate: true,
    }
}

fn base_call(query_params: HashMap<String, String>) -> OperationCall {
    OperationCall {
        pagination_url: None,
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
    let page2_url = format!("{base}/items-next?page=2&tag=a%2Bb&tag=c");
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
        .and(path("/items-next"))
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

    let dir = tempfile::tempdir().unwrap();
    let mut ctx = base_ctx();
    ctx.cache_config = Some(aperture_cli::response_cache::CacheConfig {
        cache_dir: dir.path().to_path_buf(),
        ..Default::default()
    });
    let mut buf: Vec<u8> = Vec::new();
    let count = execute_paginated(&spec, base_call(HashMap::new()), ctx.clone(), &mut buf)
        .await
        .expect("should succeed");

    assert_eq!(count, 3, "should collect 3 items across 2 pages");
    let mut cached = Vec::new();
    let cached_count = execute_paginated(&spec, base_call(HashMap::new()), ctx, &mut cached)
        .await
        .unwrap();
    assert_eq!(cached_count, count);
    assert_eq!(cached, buf);
    let items = parse_ndjson(&buf);
    assert_eq!(items[0]["id"], 1);
    assert_eq!(items[2]["id"], 3);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[1].url.path(), "/items-next");
    let tags: Vec<_> = requests[1]
        .url
        .query_pairs()
        .filter(|(key, _)| key == "tag")
        .map(|(_, value)| value.into_owned())
        .collect();
    assert_eq!(tags, vec!["a+b", "c"]);
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

fn strategy_info(strategy: PaginationStrategy) -> PaginationInfo {
    PaginationInfo {
        strategy,
        cursor_field: Some("next_cursor".into()),
        cursor_param: Some("cursor".into()),
        page_param: None,
        limit_param: None,
    }
}

#[tokio::test]
async fn cross_origin_links_are_rejected_before_request() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", format!("<{}/stolen>; rel=next", other.uri()))
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let error = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("same-origin"));
    assert!(other.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn repeated_cursor_reports_incomplete_traversal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [1], "next_cursor": "again"})),
        )
        .mount(&server)
        .await;
    let spec = make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::Cursor));
    let error = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("loop"));
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn repeated_link_reports_incomplete_traversal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", "</items>; rel=next")
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let error = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("loop"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn full_page_cap_reports_incomplete_traversal() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([1])))
        .mount(&server)
        .await;
    let spec = make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::Offset));
    let call = base_call(HashMap::from([("limit".into(), "1".into())]));
    let error = execute_paginated(&spec, call, base_ctx(), &mut Vec::new())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cap"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1000);
}

struct ClosedOutput;
impl std::io::Write for ClosedOutput {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn broken_pipe_stops_before_next_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [1], "next_cursor": "again"})),
        )
        .mount(&server)
        .await;
    let spec = make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::Cursor));
    execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut ClosedOutput,
    )
    .await
    .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn relative_link_without_query_retains_same_origin_headers() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", "<items-next>; rel=next")
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    Mock::given(path("/items-next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([2])))
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let mut call = base_call(HashMap::new());
    call.custom_headers
        .push("Authorization: Bearer synthetic-pagination-test".into());
    let count = execute_paginated(&spec, call, base_ctx(), &mut Vec::new())
        .await
        .unwrap();
    assert_eq!(count, 2);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[1].url.path(), "/items-next");
    assert_eq!(
        requests[1].headers.get("authorization").unwrap(),
        "Bearer synthetic-pagination-test"
    );
}

#[tokio::test]
async fn buffered_broken_pipe_stops_before_next_request() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data": [1], "next_cursor": "again"})),
        )
        .mount(&server)
        .await;
    let spec = make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::Cursor));
    let mut output = std::io::BufWriter::new(ClosedOutput);
    execute_paginated(&spec, base_call(HashMap::new()), base_ctx(), &mut output)
        .await
        .unwrap();
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn link_commas_and_relation_lists_do_not_truncate_results() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header(
                    "link",
                    r#"</items-next?tag=a,b>; title="two, pages"; rel="prev next""#,
                )
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    Mock::given(path("/items-next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([2])))
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let count = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap();
    assert_eq!(count, 2);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests[1].url.query(), Some("tag=a,b"));
}

#[tokio::test]
async fn ambiguous_next_links_fail_before_another_request() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", "</a>; rel=next, </b>; rel=next")
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let error = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("Ambiguous"), "{error}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn pagination_redirects_cannot_bypass_link_origin_policy() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    for target in [
        format!("{}/stolen", other.uri()),
        format!("{}/next", server.uri()),
    ] {
        server.reset().await;
        Mock::given(path("/items"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", target))
            .mount(&server)
            .await;
        let spec =
            make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
        let mut call = base_call(HashMap::new());
        call.custom_headers
            .push("X-Api-Key: synthetic-review-test".into());
        // The SDK entry point must enforce pagination policy even if this flag
        // was not set by CLI translation.
        let mut ctx = base_ctx();
        ctx.auto_paginate = false;
        assert!(execute_paginated(&spec, call, ctx, &mut Vec::new())
            .await
            .is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        assert!(other.received_requests().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn next_url_variants_replace_original_query_without_leaking_query_auth() {
    let server = MockServer::start().await;
    let origin = reqwest::Url::parse(&server.uri()).unwrap();
    for target in [
        format!("{}/next?tag=a&tag=b", server.uri()),
        "/next?tag=a&tag=b".into(),
        "next?tag=a&tag=b".into(),
        format!(
            "//{}:{}/next?tag=a&tag=b",
            origin.host_str().unwrap(),
            origin.port().unwrap()
        ),
        "?tag=a&tag=b".into(),
        "/next".into(),
    ] {
        server.reset().await;
        Mock::given(query_param("api_key", "synthetic-query-auth"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("link", format!("<{target}>; rel=next"))
                    .set_body_json(serde_json::json!([1])),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([2])))
            .with_priority(10)
            .mount(&server)
            .await;
        let spec =
            make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
        let call = base_call(HashMap::from([(
            "api_key".into(),
            "synthetic-query-auth".into(),
        )]));
        assert_eq!(
            execute_paginated(&spec, call, base_ctx(), &mut Vec::new())
                .await
                .unwrap(),
            2
        );
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[1]
            .url
            .query_pairs()
            .any(|(key, _)| key == "api_key"));
        assert_eq!(
            requests[1].url.path(),
            if target.starts_with('?') {
                "/items"
            } else {
                "/next"
            }
        );
        assert_eq!(
            requests[1].url.query(),
            if target == "/next" {
                None
            } else {
                Some("tag=a&tag=b")
            }
        );
    }
}

#[tokio::test]
async fn exact_complete_page_cap_succeeds() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let server = MockServer::start().await;
    let pages = Arc::new(AtomicUsize::new(0));
    let seen = pages.clone();
    Mock::given(method("GET"))
        .respond_with(move |_: &wiremock::Request| {
            let page = seen.fetch_add(1, Ordering::SeqCst) + 1;
            let next = (page < 1000).then(|| page.to_string());
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"data":[page], "next_cursor":next}))
        })
        .mount(&server)
        .await;
    let spec = make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::Cursor));
    assert_eq!(
        execute_paginated(
            &spec,
            base_call(HashMap::new()),
            base_ctx(),
            &mut Vec::new()
        )
        .await
        .unwrap(),
        1000
    );
    assert_eq!(pages.load(Ordering::SeqCst), 1000);
    assert_eq!(server.received_requests().await.unwrap().len(), 1000);
}

#[tokio::test]
async fn invalid_next_urls_and_duplicate_link_fields_fail_closed() {
    let server = MockServer::start().await;
    let origin = reqwest::Url::parse(&server.uri()).unwrap();
    for target in [
        "#fragment".into(),
        format!(
            "http://user:password@{}:{}/next",
            origin.host_str().unwrap(),
            origin.port().unwrap()
        ),
        format!(
            "https://{}:{}/next",
            origin.host_str().unwrap(),
            origin.port().unwrap()
        ),
    ] {
        server.reset().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("link", format!("<{target}>; rel=next"))
                    .set_body_json(serde_json::json!([1])),
            )
            .mount(&server)
            .await;
        let spec =
            make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
        assert!(execute_paginated(
            &spec,
            base_call(HashMap::new()),
            base_ctx(),
            &mut Vec::new()
        )
        .await
        .is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
    server.reset().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("link", "</a>; rel=next")
                .append_header("link", "</b>; rel=next")
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let error = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("Ambiguous"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn malformed_link_encoding_is_not_silent_completion() {
    let server = MockServer::start().await;
    let value = reqwest::header::HeaderValue::from_bytes(b"</next>; rel=next; title=\xff").unwrap();
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", value)
                .set_body_json(serde_json::json!([1])),
        )
        .mount(&server)
        .await;
    let spec =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let error = execute_paginated(
        &spec,
        base_call(HashMap::new()),
        base_ctx(),
        &mut Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("encoding"), "{error}");
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// A warmed auto-follow client must never serve either strict entry point.
#[tokio::test]
async fn warmed_normal_client_cannot_forward_pagination_api_key_on_redirect() {
    use aperture_cli::engine::executor::execute;
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    Mock::given(path("/warm"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;
    for target in [
        format!("{}/stolen", other.uri()),
        format!("{}/next", server.uri()),
    ] {
        server.reset().await;
        Mock::given(path("/warm"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(path("/items"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", target))
            .mount(&server)
            .await;
        let mut cached =
            make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
        let ctx = ExecutionContext {
            auto_paginate: false,
            proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
            ..Default::default()
        };
        cached.commands[0].path = "/warm".into();
        execute(&cached, base_call(HashMap::new()), ctx.clone())
            .await
            .unwrap();
        cached.commands[0].path = "/items".into();
        let mut call = base_call(HashMap::new());
        call.custom_headers
            .push("X-Api-Key: synthetic-warmed-key".into());
        assert!(
            execute_paginated(&cached, call.clone(), ctx.clone(), &mut Vec::new())
                .await
                .is_err()
        );
        call.pagination_url =
            Some(reqwest::Url::parse(&format!("{}/items", server.uri())).unwrap());
        assert!(execute(&cached, call, ctx).await.is_err());
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[1].headers.contains_key("x-api-key"));
        assert!(requests[2].headers.contains_key("x-api-key"));
        assert!(other.received_requests().await.unwrap().is_empty());
    }
}

/// Normal redirect responses cannot be read through the strict cache partition.
/// Direct strict responses still cache their Link headers and subsequent pages.
#[tokio::test]
async fn warmed_redirect_cache_cannot_bypass_strict_pagination_boundary() {
    use aperture_cli::engine::executor::execute;
    use aperture_cli::invocation::ExecutionResult;
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/landing", other.uri())),
        )
        .mount(&server)
        .await;
    Mock::given(path("/landing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([1])))
        .mount(&other)
        .await;
    let cached =
        make_spec_with_pagination(&server.uri(), strategy_info(PaginationStrategy::LinkHeader));
    let dir = tempfile::tempdir().unwrap();
    let ctx = ExecutionContext {
        auto_paginate: false,
        proxy_override: aperture_cli::invocation::ProxyOverride::Disable,
        cache_config: Some(aperture_cli::response_cache::CacheConfig {
            enabled: true,
            cache_dir: dir.path().into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert!(matches!(
        execute(&cached, base_call(HashMap::new()), ctx.clone())
            .await
            .unwrap(),
        ExecutionResult::Success { .. }
    ));
    assert!(matches!(
        execute(&cached, base_call(HashMap::new()), ctx.clone())
            .await
            .unwrap(),
        ExecutionResult::Cached { .. }
    ));
    assert!(execute_paginated(
        &cached,
        base_call(HashMap::new()),
        ctx.clone(),
        &mut Vec::new()
    )
    .await
    .is_err());
    let mut override_call = base_call(HashMap::new());
    override_call.pagination_url =
        Some(reqwest::Url::parse(&format!("{}/items", server.uri())).unwrap());
    assert!(execute(&cached, override_call, ctx.clone()).await.is_err());
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    assert_eq!(other.received_requests().await.unwrap().len(), 1);
    server.reset().await;
    Mock::given(path("/items"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", "</page2>; rel=next")
                .set_body_json(serde_json::json!([1])),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/page2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([2])))
        .expect(1)
        .mount(&server)
        .await;
    for _ in 0..2 {
        let mut output = Vec::new();
        assert_eq!(
            execute_paginated(&cached, base_call(HashMap::new()), ctx.clone(), &mut output)
                .await
                .unwrap(),
            2
        );
        assert_eq!(String::from_utf8(output).unwrap(), "1\n2\n");
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}
