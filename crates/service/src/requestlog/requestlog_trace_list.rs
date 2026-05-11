use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

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
    read_trace_file_tail_from_path(&trace_file_path_from_env())
}

fn read_trace_file_tail_from_path(path: &Path) -> Result<String, String> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(err) => return Err(format!("read gateway trace metadata failed: {err}")),
    };
    let file_len = metadata.len();
    let read_len = file_len.min(MAX_TRACE_FILE_BYTES);
    let start = file_len.saturating_sub(read_len);
    let mut file =
        File::open(path).map_err(|err| format!("open gateway trace log failed: {err}"))?;
    file.seek(SeekFrom::Start(start))
        .map_err(|err| format!("seek gateway trace log failed: {err}"))?;
    let mut buffer = vec![0; read_len as usize];
    file.read_exact(&mut buffer)
        .map_err(|err| format!("read gateway trace log tail failed: {err}"))?;
    if start > 0 {
        if let Some(newline_pos) = buffer.iter().position(|byte| *byte == b'\n') {
            buffer.drain(..=newline_pos);
        }
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
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
    use std::fs;

    use super::{
        normalize_page_size, parse_trace_line, read_trace_file_tail_from_path,
        MAX_GATEWAY_TRACE_PAGE_SIZE, MAX_TRACE_FILE_BYTES,
    };

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
    fn parser_documents_space_delimited_value_limitation() {
        let entry = parse_trace_line("ts=123 event=TEST trace_id=trc_1 reason=hello world")
            .expect("trace entry");
        assert_eq!(
            entry.fields.get("reason").map(String::as_str),
            Some("hello")
        );
        assert!(entry.raw.ends_with("reason=hello world"));
    }

    #[test]
    fn reads_only_tail_and_drops_partial_first_line() {
        let path = std::env::temp_dir().join(format!(
            "codexmanager-trace-tail-{}-{}.log",
            std::process::id(),
            MAX_TRACE_FILE_BYTES
        ));
        let mut content = "partial_line_without_newline"
            .repeat((MAX_TRACE_FILE_BYTES as usize / "partial_line_without_newline".len()) + 2);
        content.push_str("\nts=999 event=ROUTE_CONVERSATION_DECISION trace_id=trc_tail route_source=prompt_cache_key\n");
        fs::write(&path, content).expect("write trace file");

        let tail = read_trace_file_tail_from_path(&path).expect("read trace tail");
        let _ = fs::remove_file(&path);

        assert!(tail.len() <= MAX_TRACE_FILE_BYTES as usize);
        assert!(!tail.starts_with("partial_line_without_newline"));
        assert!(tail.contains("trace_id=trc_tail"));
    }

    #[test]
    fn bounds_trace_page_size() {
        assert_eq!(normalize_page_size(0), 20);
        assert_eq!(normalize_page_size(999), MAX_GATEWAY_TRACE_PAGE_SIZE);
    }
}
