use tiny_http::{Request, Response};

const LOCAL_PROPS_BODY: &str = "{}";

pub(super) fn is_props_probe_path(path: &str) -> bool {
    path == "/v1/props"
        || path.starts_with("/v1/props?")
        || path == "/props"
        || path.starts_with("/props?")
}

pub(super) fn maybe_respond_local_props_pre_auth(
    request: Request,
    trace_id: &str,
    original_path: &str,
    request_path_for_log: &str,
    request_method: &str,
) -> Result<Option<Request>, String> {
    if !request_method.eq_ignore_ascii_case("GET") || !is_props_probe_path(original_path) {
        return Ok(Some(request));
    }

    super::trace_log::log_request_start(
        trace_id,
        "-",
        request_method,
        request_path_for_log,
        None,
        None,
        None,
        false,
        "http",
        "-",
    );
    super::trace_log::log_attempt_result(trace_id, "-", None, 200, None);
    super::trace_log::log_request_final(trace_id, 200, None, None, None, 0);
    super::record_gateway_request_outcome(request_path_for_log, 200, None);
    if let Some(storage) = super::open_storage() {
        super::write_request_log(
            &storage,
            super::request_log::RequestLogTraceContext {
                trace_id: Some(trace_id),
                original_path: Some(request_path_for_log),
                adapted_path: Some(request_path_for_log),
                response_adapter: Some(super::ResponseAdapter::Passthrough),
                ..Default::default()
            },
            None,
            None,
            request_path_for_log,
            request_method,
            None,
            None,
            None,
            Some(200),
            super::request_log::RequestLogUsage::default(),
            None,
            None,
        );
    }

    let response = super::error_response::with_trace_id_header(
        Response::from_string(LOCAL_PROPS_BODY)
            .with_status_code(200)
            .with_header(
                tiny_http::Header::from_bytes(
                    b"content-type".as_slice(),
                    b"application/json".as_slice(),
                )
                .map_err(|_| "build content-type header failed".to_string())?,
            ),
        Some(trace_id),
    );
    let _ = request.respond(response);
    Ok(None)
}

pub(super) fn maybe_respond_local_props(
    request: Request,
    trace_id: &str,
    key_id: &str,
    protocol_type: &str,
    original_path: &str,
    path: &str,
    response_adapter: super::ResponseAdapter,
    request_method: &str,
    model_for_log: Option<&str>,
    reasoning_for_log: Option<&str>,
    storage: &codexmanager_core::storage::Storage,
) -> Result<Option<Request>, String> {
    if !request_method.eq_ignore_ascii_case("GET") || !is_props_probe_path(path) {
        return Ok(Some(request));
    }

    let context = super::local_response::LocalResponseContext {
        trace_id,
        key_id,
        protocol_type,
        original_path,
        path,
        response_adapter,
        request_method,
        model_for_log,
        reasoning_for_log,
        storage,
    };
    super::local_response::respond_local_json(
        request,
        &context,
        LOCAL_PROPS_BODY.to_string(),
        super::request_log::RequestLogUsage::default(),
    )?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::is_props_probe_path;

    #[test]
    fn props_probe_path_matches_versioned_and_legacy_paths() {
        assert!(is_props_probe_path("/v1/props"));
        assert!(is_props_probe_path("/v1/props?verbose=true"));
        assert!(is_props_probe_path("/props"));
        assert!(is_props_probe_path("/props?verbose=true"));
    }

    #[test]
    fn props_probe_path_does_not_match_other_paths() {
        assert!(!is_props_probe_path("/v1/responses"));
        assert!(!is_props_probe_path("/v1/props-extra"));
        assert!(!is_props_probe_path("/v1/props/extra"));
        assert!(!is_props_probe_path("/foo/props"));
    }
}
