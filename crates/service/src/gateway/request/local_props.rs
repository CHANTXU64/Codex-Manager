use tiny_http::Request;

const LOCAL_PROPS_BODY: &str = "{}";

fn is_props_probe_path(path: &str) -> bool {
    path == "/v1/props"
        || path.starts_with("/v1/props?")
        || path == "/props"
        || path.starts_with("/props?")
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
