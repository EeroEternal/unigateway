pub mod anthropic;
pub mod openai;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use crate::pool::RequestId;
use crate::response::{AttemptReport, AttemptStatus, RequestKind, RequestReport, TokenUsage};
use crate::transport::HttpTransport;

pub use anthropic::AnthropicDriver;
pub use openai::OpenAiCompatibleDriver;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn builtin_drivers(transport: Arc<dyn HttpTransport>) -> Vec<Arc<dyn crate::ProviderDriver>> {
    vec![
        Arc::new(OpenAiCompatibleDriver::new(transport.clone())),
        Arc::new(AnthropicDriver::new(transport)),
    ]
}

pub(crate) fn next_request_id() -> String {
    format!("req-{}", REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed))
}

pub(crate) fn build_single_attempt_report(
    endpoint_id: &str,
    latency_ms: u64,
    error: Option<String>,
) -> AttemptReport {
    AttemptReport {
        endpoint_id: endpoint_id.to_string(),
        status: if error.is_some() {
            AttemptStatus::Failed
        } else {
            AttemptStatus::Succeeded
        },
        latency_ms,
        error,
        error_kind: None,
    }
}

pub(crate) fn build_request_report(
    endpoint: &crate::DriverEndpointContext,
    started_at: SystemTime,
    finished_at: SystemTime,
    usage: Option<TokenUsage>,
    kind: RequestKind,
    request_id: Option<RequestId>,
) -> RequestReport {
    let latency_ms = finished_at
        .duration_since(started_at)
        .unwrap_or_default()
        .as_millis() as u64;

    let request_id = request_id.unwrap_or_else(next_request_id);

    RequestReport {
        request_id: request_id.clone(),
        correlation_id: request_id,
        pool_id: endpoint.metadata.get("pool_id").cloned(),
        selected_endpoint_id: endpoint.endpoint_id.clone(),
        selected_provider: endpoint.provider_kind,
        kind,
        attempts: vec![build_single_attempt_report(
            &endpoint.endpoint_id,
            latency_ms,
            None,
        )],
        usage,
        latency_ms,
        started_at,
        finished_at,
        error_kind: None,
        stream: None,
        metadata: endpoint.metadata.clone(),
    }
}

pub(crate) fn output_text_from_openai_message(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }

    let parts = value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedSseEvent {
    pub event: Option<String>,
    pub data: String,
}

pub(crate) fn parse_sse_frame(frame_bytes: &[u8]) -> Option<ParsedSseEvent> {
    let frame = String::from_utf8_lossy(frame_bytes);
    let mut event = None;
    let mut data_lines = Vec::new();

    for line in frame.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_string());
            continue;
        }

        if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        }
    }

    if data_lines.is_empty() {
        None
    } else {
        Some(ParsedSseEvent {
            event,
            data: data_lines.join("\n"),
        })
    }
}

pub(crate) fn drain_sse_frames(buffer: &mut Vec<u8>) -> Vec<ParsedSseEvent> {
    let mut frames = Vec::new();

    while let Some((index, delimiter_len)) = find_sse_delimiter(buffer) {
        let frame_bytes: Vec<u8> = buffer.drain(..index).collect();
        buffer.drain(..delimiter_len);

        if let Some(frame) = parse_sse_frame(&frame_bytes) {
            frames.push(frame);
        }
    }

    frames
}

fn find_sse_delimiter(buffer: &[u8]) -> Option<(usize, usize)> {
    if buffer.len() < 2 {
        return None;
    }

    for index in 0..(buffer.len() - 1) {
        if buffer[index] == b'\n' && buffer[index + 1] == b'\n' {
            return Some((index, 2));
        }

        if index + 3 < buffer.len()
            && buffer[index] == b'\r'
            && buffer[index + 1] == b'\n'
            && buffer[index + 2] == b'\r'
            && buffer[index + 3] == b'\n'
        {
            return Some((index, 4));
        }
    }

    None
}
