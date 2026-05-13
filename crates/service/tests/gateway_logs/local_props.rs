use super::*;

fn seed_props_probe_state(storage: &Storage, platform_key: &str, now: i64) {
    storage
        .insert_api_key(&ApiKey {
            id: "gk_props_probe".to_string(),
            name: Some("props-probe".to_string()),
            model_slug: Some("gpt-5.3-codex".to_string()),
            reasoning_effort: None,
            service_tier: None,
            rotation_strategy: "account_rotation".to_string(),
            aggregate_api_id: None,
            account_plan_filter: None,
            aggregate_api_url: None,
            client_type: "codex".to_string(),
            protocol_type: "openai_compat".to_string(),
            auth_scheme: "authorization_bearer".to_string(),
            upstream_base_url: None,
            static_headers_json: None,
            key_hash: hash_platform_key_for_test(platform_key),
            status: "active".to_string(),
            created_at: now,
            last_used_at: None,
        })
        .expect("insert api key");
    storage
        .upsert_api_key_active_account(&ApiKeyActiveAccount {
            key_id: "gk_props_probe".to_string(),
            active_account_id: "acc_existing".to_string(),
            active_started_at: now - 60,
            last_used_at: now - 30,
            consecutive_real_errors: 0,
            last_switch_reason: Some("preseed".to_string()),
            updated_at: now - 30,
        })
        .expect("seed active account");
}

#[test]
fn gateway_local_props_probe_returns_json_without_upstream_or_account_routing() {
    let _lock = test_env_guard();
    let dir = new_test_dir("codexmanager-gateway-local-props");
    let db_path: PathBuf = dir.join("codexmanager.db");
    let _db_guard = EnvGuard::set("CODEXMANAGER_DB_PATH", db_path.to_string_lossy().as_ref());

    let (upstream_addr, upstream_rx, upstream_join) =
        start_mock_upstream_sequence_lenient(Vec::new(), Duration::from_millis(200));
    let upstream_base = format!("http://{upstream_addr}/backend-api/codex");
    let _upstream_guard = EnvGuard::set("CODEXMANAGER_UPSTREAM_BASE_URL", &upstream_base);

    let storage = Storage::open(&db_path).expect("open db");
    storage.init().expect("init db");
    let now = now_ts();
    let platform_key = "pk_props_probe";
    seed_props_probe_state(&storage, platform_key, now);

    let server = TestServer::start();
    for path in ["/v1/props", "/props", "/v1/props?xxx=yyy"] {
        let headers = if path == "/v1/props" {
            Vec::new()
        } else {
            vec![("Authorization", "Bearer pk_props_probe")]
        };
        let (status, response_headers, body) = get_http_raw(&server.addr, path, &headers);

        assert_eq!(status, 200, "path {path} body: {body}");
        assert_eq!(body, "{}");
        assert_eq!(
            response_headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    assert!(
        upstream_rx.try_recv().is_err(),
        "local props probe should not reach mock upstream"
    );
    upstream_join.join().expect("join upstream");

    let active = storage
        .get_api_key_active_account("gk_props_probe")
        .expect("read active account")
        .expect("active account exists");
    assert_eq!(active.active_account_id, "acc_existing");
    assert_eq!(active.last_switch_reason.as_deref(), Some("preseed"));

    let logs = storage
        .list_request_logs(None, 100)
        .expect("list request logs");
    for path in ["/v1/props", "/props", "/v1/props?xxx=yyy"] {
        let log = logs
            .iter()
            .find(|item| item.request_path == path)
            .unwrap_or_else(|| panic!("missing props request log for {path}: {logs:?}"));
        assert_eq!(log.status_code, Some(200));
        assert_eq!(log.method, "GET");
        assert!(
            log.key_id.is_none(),
            "props probe should not load an api key"
        );
        assert!(
            log.account_id.is_none(),
            "props probe should not select an account"
        );
        assert!(
            log.initial_account_id.is_none(),
            "props probe should not write attempted account state"
        );
    }
}
