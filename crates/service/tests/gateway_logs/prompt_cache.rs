use super::*;
use codexmanager_core::storage::{ConversationBinding, UsageSnapshotRecord};

const MODEL: &str = "gpt-5.3-codex";
const PROTOCOL_OPENAI_COMPAT: &str = "openai_compat";

fn prompt_cache_route_id(platform_key_hash: &str, prompt_cache_key: &str) -> String {
    let digest = Sha256::digest(
        format!(
            "pck:v1\0{platform_key_hash}\0{PROTOCOL_OPENAI_COMPAT}\0{}",
            prompt_cache_key.trim()
        )
        .as_bytes(),
    );
    format!(
        "pck:v1:{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
        digest[8], digest[9], digest[10], digest[11], digest[12], digest[13], digest[14], digest[15]
    )
}

fn ok_response(id: &str) -> String {
    serde_json::json!({
        "id": id,
        "model": MODEL,
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": "ok" }]
        }],
        "usage": {
            "input_tokens": 3,
            "output_tokens": 1,
            "total_tokens": 4
        }
    })
    .to_string()
}

fn seed_openai_compat_gateway(storage: &Storage, platform_key: &str, key_id: &str) -> String {
    let now = now_ts();
    seed_model_catalog_models(storage, &[MODEL]);

    for (id, sort) in [("acc_prompt_cache_a", 0_i64), ("acc_prompt_cache_b", 1_i64)] {
        storage
            .insert_account(&Account {
                id: id.to_string(),
                label: id.to_string(),
                issuer: "https://auth.openai.com".to_string(),
                chatgpt_account_id: Some(format!("chatgpt_{id}")),
                workspace_id: None,
                group_name: None,
                sort,
                status: "active".to_string(),
                created_at: now + sort,
                updated_at: now + sort,
            })
            .expect("insert account");
        storage
            .insert_token(&Token {
                account_id: id.to_string(),
                id_token: String::new(),
                access_token: format!("access_{id}"),
                refresh_token: String::new(),
                api_key_access_token: Some(format!("api_access_{id}")),
                last_refresh: now,
            })
            .expect("insert token");
        storage
            .insert_usage_snapshot(&UsageSnapshotRecord {
                account_id: id.to_string(),
                used_percent: Some(10.0),
                window_minutes: Some(300),
                resets_at: None,
                secondary_used_percent: None,
                secondary_window_minutes: None,
                secondary_resets_at: None,
                credits_json: None,
                captured_at: now,
            })
            .expect("insert usage snapshot");
    }

    let platform_key_hash = hash_platform_key_for_test(platform_key);
    storage
        .insert_api_key(&ApiKey {
            id: key_id.to_string(),
            name: Some(key_id.to_string()),
            model_slug: Some(MODEL.to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: PROTOCOL_OPENAI_COMPAT.to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: platform_key_hash.clone(),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");

    platform_key_hash
}

fn post_responses(server_addr: &str, platform_key: &str, path: &str, body: serde_json::Value) {
    post_responses_with_headers(server_addr, platform_key, path, body, &[]);
}

fn post_responses_with_headers(
    server_addr: &str,
    platform_key: &str,
    path: &str,
    body: serde_json::Value,
    extra_headers: &[(&str, &str)],
) {
    let body = serde_json::to_string(&body).expect("serialize request");
    let authorization = format!("Bearer {platform_key}");
    let mut headers = vec![
        ("Content-Type", "application/json"),
        ("Authorization", authorization.as_str()),
    ];
    headers.extend_from_slice(extra_headers);
    let (status, gateway_body) = post_http_raw(server_addr, path, &body, &headers);
    assert_eq!(status, 200, "gateway response body: {gateway_body}");
}

fn auth_account(captured: &CapturedUpstreamRequest) -> &str {
    let auth = captured
        .headers
        .get("authorization")
        .map(String::as_str)
        .unwrap_or_default();
    if auth.contains("access_acc_prompt_cache_a") {
        "acc_prompt_cache_a"
    } else if auth.contains("access_acc_prompt_cache_b") {
        "acc_prompt_cache_b"
    } else {
        panic!("unexpected upstream authorization header: {auth}");
    }
}

fn read_trace_log(dir: &std::path::Path) -> String {
    let path = dir.join("gateway-trace.log");
    for _ in 0..40 {
        if let Ok(content) = fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return content;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    fs::read_to_string(&path).unwrap_or_default()
}

#[test]
fn gateway_prompt_cache_binding_reuses_account_for_previous_response_chain() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-pck-reuse-chain");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _route_guard = EnvGuard::set("CODEXMANAGER_ROUTE_STRATEGY", "balanced");

    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (200, ok_response("resp_pck_first")),
        (200, ok_response("resp_pck_second")),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let platform_key = "pk_prompt_cache_reuse_chain";
    let key_hash =
        seed_openai_compat_gateway(&storage, platform_key, "gk_prompt_cache_reuse_chain");
    let prompt_cache_key = "client-thread-reuse-123456";
    let route_id = prompt_cache_route_id(&key_hash, prompt_cache_key);

    let first_server = codexmanager_service::start_one_shot_server().expect("start first server");
    post_responses(
        &first_server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "first",
            "stream": false,
            "prompt_cache_key": prompt_cache_key
        }),
    );
    first_server.join();

    let binding = storage
        .get_conversation_binding(&key_hash, &route_id)
        .expect("load pck binding")
        .expect("pck binding should be created by first request");
    assert_eq!(binding.account_id, "acc_prompt_cache_a");
    assert_eq!(binding.thread_anchor, route_id);

    let second_server = codexmanager_service::start_one_shot_server().expect("start second server");
    post_responses(
        &second_server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "follow-up",
            "stream": false,
            "previous_response_id": "resp_pck_first",
            "prompt_cache_key": prompt_cache_key
        }),
    );
    second_server.join();

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive second upstream request");
    upstream_join.join().expect("join mock upstream");

    assert_eq!(auth_account(&first), "acc_prompt_cache_a");
    assert_eq!(
        auth_account(&second),
        "acc_prompt_cache_a",
        "existing-only previous_response_id requests must use the existing pck binding instead of balanced rotation"
    );

    let first_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&first))
            .expect("parse first upstream body");
    let second_body: serde_json::Value =
        serde_json::from_slice(&decode_upstream_request_body(&second))
            .expect("parse second upstream body");
    assert_eq!(
        first_body.get("prompt_cache_key").and_then(serde_json::Value::as_str),
        Some(prompt_cache_key),
        "client pck should be forwarded as the body cache key, not replaced by the local pck route id"
    );
    assert_eq!(
        second_body
            .get("prompt_cache_key")
            .and_then(serde_json::Value::as_str),
        Some(prompt_cache_key),
        "existing-only pck route id must stay route-only and not become upstream prompt_cache_key"
    );

    let trace_log = read_trace_log(&dir);
    assert!(
        trace_log.contains("event=ROUTE_CONVERSATION_DECISION")
            && trace_log.contains("route_source=prompt_cache_key"),
        "prompt-cache route decisions should be written to gateway trace log, got: {trace_log}"
    );
    assert!(
        trace_log.contains("event=CONVERSATION_BINDING_RECORD")
            && trace_log.contains("action=create_initial"),
        "prompt-cache binding creation should be written to gateway trace log, got: {trace_log}"
    );
    assert!(
        trace_log.contains("route_source=prompt_cache_key_existing_only")
            && trace_log.contains("action=touch_existing"),
        "existing-only prompt-cache follow-up should log route source and binding touch, got: {trace_log}"
    );
}

#[test]
fn gateway_previous_response_without_existing_pck_binding_does_not_create_binding() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-pck-existing-only-no-create");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _route_guard = EnvGuard::set("CODEXMANAGER_ROUTE_STRATEGY", "balanced");

    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence(vec![(200, ok_response("resp_existing_only"))]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let platform_key = "pk_prompt_cache_existing_only_no_create";
    let key_hash = seed_openai_compat_gateway(
        &storage,
        platform_key,
        "gk_prompt_cache_existing_only_no_create",
    );
    let prompt_cache_key = "client-thread-missing-binding-123456";
    let route_id = prompt_cache_route_id(&key_hash, prompt_cache_key);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    post_responses(
        &server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "follow-up without known binding",
            "stream": false,
            "previous_response_id": "resp_missing_local_binding",
            "prompt_cache_key": prompt_cache_key
        }),
    );
    server.join();

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(auth_account(&captured), "acc_prompt_cache_a");

    let actual = storage
        .get_conversation_binding(&key_hash, &route_id)
        .expect("load pck binding");
    assert!(
        actual.is_none(),
        "PromptCacheKeyExistingOnly must not create a binding from a previous_response_id request"
    );
}

#[test]
fn gateway_prompt_cache_route_path_filter_accepts_query_and_rejects_prefix() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-pck-path-filter");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (200, ok_response("resp_query_path")),
        (200, ok_response("resp_prefix_path")),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let platform_key = "pk_prompt_cache_path_filter";
    let key_hash =
        seed_openai_compat_gateway(&storage, platform_key, "gk_prompt_cache_path_filter");
    let query_prompt_cache_key = "client-thread-query-path-123456";
    let prefix_prompt_cache_key = "client-thread-prefix-path-123456";
    let query_route_id = prompt_cache_route_id(&key_hash, query_prompt_cache_key);
    let prefix_route_id = prompt_cache_route_id(&key_hash, prefix_prompt_cache_key);

    let query_server = codexmanager_service::start_one_shot_server().expect("start query server");
    post_responses(
        &query_server.addr,
        platform_key,
        "/v1/responses?trace=1",
        serde_json::json!({
            "model": MODEL,
            "input": "query path",
            "stream": false,
            "prompt_cache_key": query_prompt_cache_key
        }),
    );
    query_server.join();

    let prefix_server = codexmanager_service::start_one_shot_server().expect("start prefix server");
    post_responses(
        &prefix_server.addr,
        platform_key,
        "/v1/responsesxxx",
        serde_json::json!({
            "model": MODEL,
            "input": "prefix path",
            "stream": false,
            "prompt_cache_key": prefix_prompt_cache_key
        }),
    );
    prefix_server.join();

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive query upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive prefix upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(first.path, "/backend-api/codex/responses?trace=1");
    assert_eq!(second.path, "/backend-api/codex/responsesxxx");

    assert!(
        storage
            .get_conversation_binding(&key_hash, &query_route_id)
            .expect("load query binding")
            .is_some(),
        "official responses path with query string should create pck binding"
    );
    assert!(
        storage
            .get_conversation_binding(&key_hash, &prefix_route_id)
            .expect("load prefix binding")
            .is_none(),
        "non-official /v1/responsesxxx prefix path must not create pck binding"
    );
}

#[test]
fn gateway_prompt_cache_manual_preferred_does_not_migrate_existing_binding() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-pck-manual-no-migrate");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (200, ok_response("resp_manual_seed")),
        (200, ok_response("resp_manual_override")),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let mut storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let platform_key = "pk_prompt_cache_manual_no_migrate";
    let key_hash =
        seed_openai_compat_gateway(&storage, platform_key, "gk_prompt_cache_manual_no_migrate");
    let prompt_cache_key = "client-thread-manual-preferred-123456";
    let route_id = prompt_cache_route_id(&key_hash, prompt_cache_key);

    let first_server = codexmanager_service::start_one_shot_server().expect("start first server");
    post_responses(
        &first_server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "seed binding",
            "stream": false,
            "prompt_cache_key": prompt_cache_key
        }),
    );
    first_server.join();

    let seed_binding = storage
        .get_conversation_binding(&key_hash, &route_id)
        .expect("load seed binding")
        .expect("seed binding should exist");
    assert_eq!(seed_binding.account_id, "acc_prompt_cache_a");

    storage
        .set_preferred_account(Some("acc_prompt_cache_b"))
        .expect("set manual preferred account");

    let second_server = codexmanager_service::start_one_shot_server().expect("start second server");
    post_responses(
        &second_server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "manual preferred attempt",
            "stream": false,
            "prompt_cache_key": prompt_cache_key
        }),
    );
    second_server.join();

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive second upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(auth_account(&first), "acc_prompt_cache_a");
    assert_eq!(
        auth_account(&second),
        "acc_prompt_cache_b",
        "manual preferred account should control the current attempt order"
    );

    let final_binding = storage
        .get_conversation_binding(&key_hash, &route_id)
        .expect("load final binding")
        .expect("binding should still exist");
    assert_eq!(
        final_binding.account_id, "acc_prompt_cache_a",
        "manual preferred success must not migrate a selectable existing pck binding"
    );
}

#[test]
fn gateway_turn_state_disables_prompt_cache_route_even_when_binding_exists() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-pck-turn-state-disabled");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _route_guard = EnvGuard::set("CODEXMANAGER_ROUTE_STRATEGY", "balanced");

    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence(vec![(200, ok_response("resp_turn_state"))]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let platform_key = "pk_prompt_cache_turn_state_disabled";
    let key_hash = seed_openai_compat_gateway(
        &storage,
        platform_key,
        "gk_prompt_cache_turn_state_disabled",
    );
    let prompt_cache_key = "client-thread-turn-state-123456";
    let route_id = prompt_cache_route_id(&key_hash, prompt_cache_key);
    let now = now_ts();
    storage
        .upsert_conversation_binding(&ConversationBinding {
            platform_key_hash: key_hash.clone(),
            conversation_id: route_id.clone(),
            account_id: "acc_prompt_cache_b".to_string(),
            thread_epoch: 1,
            thread_anchor: route_id.clone(),
            status: "active".to_string(),
            last_model: Some(MODEL.to_string()),
            last_switch_reason: None,
            created_at: now,
            updated_at: now,
            last_used_at: now,
        })
        .expect("seed pck binding");

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    post_responses_with_headers(
        &server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "turn state wins",
            "stream": false,
            "prompt_cache_key": prompt_cache_key
        }),
        &[("x-codex-turn-state", "turn-state-anchor")],
    );
    server.join();

    let captured = upstream_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("receive upstream request");
    upstream_join.join().expect("join mock upstream");
    assert_eq!(
        auth_account(&captured),
        "acc_prompt_cache_a",
        "turn_state should disable pck route selection instead of using the existing pck binding"
    );

    let binding = storage
        .get_conversation_binding(&key_hash, &route_id)
        .expect("load pck binding")
        .expect("seeded pck binding should remain untouched");
    assert_eq!(binding.account_id, "acc_prompt_cache_b");
}

#[test]
fn gateway_retries_same_account_once_before_prompt_cache_failover() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-pck-same-account-retry");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());
    let _route_guard = EnvGuard::set("CODEXMANAGER_ROUTE_STRATEGY", "ordered");

    let upstream_error = serde_json::json!({
        "error": { "message": "temporary upstream failure", "type": "server_error" }
    });
    let (upstream_addr, upstream_rx, upstream_join) = start_mock_upstream_sequence(vec![
        (
            502,
            serde_json::to_string(&upstream_error).expect("serialize upstream error"),
        ),
        (200, ok_response("resp_retry_success")),
    ]);
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let platform_key = "pk_prompt_cache_same_account_retry";
    let key_hash =
        seed_openai_compat_gateway(&storage, platform_key, "gk_prompt_cache_same_account_retry");
    let prompt_cache_key = "client-thread-same-account-retry";
    let route_id = prompt_cache_route_id(&key_hash, prompt_cache_key);

    let server = codexmanager_service::start_one_shot_server().expect("start server");
    post_responses(
        &server.addr,
        platform_key,
        "/v1/responses",
        serde_json::json!({
            "model": MODEL,
            "input": "retry once before switching accounts",
            "stream": false,
            "prompt_cache_key": prompt_cache_key
        }),
    );
    server.join();

    let first = upstream_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first upstream request");
    let second = upstream_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("same-account retry request");
    assert_eq!(auth_account(&first), "acc_prompt_cache_a");
    assert_eq!(auth_account(&second), "acc_prompt_cache_a");
    assert!(upstream_rx
        .recv_timeout(Duration::from_millis(200))
        .is_err());
    upstream_join.join().expect("join upstream");

    let binding = storage
        .get_conversation_binding(&key_hash, &route_id)
        .expect("load pck binding")
        .expect("pck binding should be created after retry success");
    assert_eq!(binding.account_id, "acc_prompt_cache_a");
}
