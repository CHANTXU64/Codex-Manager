use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use codexmanager_core::rpc::types::{
    GatewayTraceListParams, GatewayTraceListResult, GatewayTraceLogEntry,
};

const DEFAULT_GATEWAY_TRACE_PAGE_SIZE: i64 = 20;
const MAX_GATEWAY_TRACE_PAGE_SIZE: i64 = 200;
const MAX_TRACE_FILE_BYTES: u64 = 4 * 1024 * 1024;
const ROUTE_TRACE_EVENTS: &[&str] = &["ROUTE_CONVERSATION_DECISION", "CONVERSATION_BINDING_RECORD"];

pub(crate) fn read_gateway_trace_logs(
    params: GatewayTraceListParams,
) -> Result<GatewayTraceListResult, String> {
    let params = params.normalized();
    let page_size = normalize_page_size(params.page_size);
    let trace_id_filter = normalize_text_filter(params.trace_id);
    let event_filter = normalize_text_filter(params.event_filter);
    let query_filter = normalize_text_filter(params.query).map(|value| value.to_ascii_lowercase());
    let content = read_trace_file_tail()?;
    let mut events = BTreeSet::new();
    let mut matched = Vec::new();

    for line in content.lines() {
        let Some(entry) = parse_trace_line(line) else {
            continue;
        };
        events.insert(entry.event.clone());
        if let Some(trace_id) = trace_id_filter.as_deref() {
            if entry.trace_id != trace_id {
                continue;
            }
        }
        if let Some(event) = event_filter.as_deref() {
            let is_route_filter = event.eq_ignore_ascii_case("route");
            let matched_event = if is_route_filter {
                ROUTE_TRACE_EVENTS.contains(&entry.event.as_str())
            } else {
                entry.event.eq_ignore_ascii_case(event)
            };
            if !matched_event {
                continue;
            }
        }
        if let Some(query) = query_filter.as_deref() {
            if !entry.raw.to_ascii_lowercase().contains(query) {
                continue;
            }
        }
        matched.push(entry);
    }

    matched.sort_by(|left, right| right.ts.cmp(&left.ts));
    let total = matched.len() as i64;
    let page = clamp_page(params.page, total, page_size);
    let offset = ((page - 1) * page_size) as usize;
    let items = matched
        .into_iter()
        .skip(offset)
        .take(page_size as usize)
        .collect();

    Ok(GatewayTraceListResult {
        items,
        total,
        page,
        page_size,
        events: events.into_iter().collect(),
    })
}

fn trace_file_path_from_env() -> PathBuf {
    if let Ok(db_path) = std::env::var("CODEXMANAGER_DB_PATH") {
        let path = PathBuf::from(db_path);
        if let Some(parent) = path.parent() {
            return parent.join("gateway-trace.log");
        }
    }
    PathBuf::from("gateway-trace.log")
}

fn read_trace_file_tail() -> Result<String, String> {
    let path = trace_file_path_from_env();
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(format!("read gateway trace metadata failed: {err}")),
    };
    let content =
        fs::read_to_string(&path).map_err(|err| format!("read gateway trace log failed: {err}"))?;
    if metadata.len() <= MAX_TRACE_FILE_BYTES {
        return Ok(content);
    }
    let keep_from = content
        .char_indices()
        .rev()
        .find(|(index, _)| content.len().saturating_sub(*index) >= MAX_TRACE_FILE_BYTES as usize)
        .map(|(index, _)| index)
        .unwrap_or(0);
    Ok(content[keep_from..].to_string())
}

fn parse_trace_line(line: &str) -> Option<GatewayTraceLogEntry> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut fields = BTreeMap::new();
    for token in trimmed.split_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        fields.insert(key.to_string(), value.to_string());
    }
    let event = fields.get("event")?.to_string();
    let trace_id = fields.get("trace_id").cloned().unwrap_or_default();
    let ts = fields
        .get("ts")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    Some(GatewayTraceLogEntry {
        ts,
        event,
        trace_id,
        fields,
        raw: trimmed.to_string(),
    })
}

fn normalize_text_filter(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("all"))
}

fn normalize_page_size(value: i64) -> i64 {
    if value < 1 {
        DEFAULT_GATEWAY_TRACE_PAGE_SIZE
    } else {
        value.min(MAX_GATEWAY_TRACE_PAGE_SIZE)
    }
}

fn clamp_page(page: i64, total: i64, page_size: i64) -> i64 {
    let normalized_page = page.max(1);
    let total_pages = if total <= 0 {
        1
    } else {
        ((total + page_size - 1) / page_size).max(1)
    };
    normalized_page.min(total_pages)
}

#[cfg(test)]
mod tests {
    use super::{normalize_page_size, parse_trace_line, MAX_GATEWAY_TRACE_PAGE_SIZE};

    #[test]
    fn parses_key_value_trace_line() {
        let entry = parse_trace_line(
            "ts=123 event=ROUTE_CONVERSATION_DECISION trace_id=trc_1 route_source=prompt_cache_key",
        )
        .expect("trace entry");
        assert_eq!(entry.ts, 123);
        assert_eq!(entry.event, "ROUTE_CONVERSATION_DECISION");
        assert_eq!(entry.trace_id, "trc_1");
        assert_eq!(
            entry.fields.get("route_source").map(String::as_str),
            Some("prompt_cache_key")
        );
    }

    #[test]
    fn bounds_trace_page_size() {
        assert_eq!(normalize_page_size(0), 20);
        assert_eq!(normalize_page_size(999), MAX_GATEWAY_TRACE_PAGE_SIZE);
    }
}
