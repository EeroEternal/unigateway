use axum::http::HeaderMap;
use serde_json::Value;

#[allow(unused_imports)]
pub use unigateway_config::routing::resolve_upstream;

/// Extract target provider hint from request headers or body.
pub fn target_provider_hint(headers: &HeaderMap, payload: &Value) -> Option<String> {
    let from_header = headers
        .get("x-unigateway-provider")
        .or_else(|| headers.get("x-target-vendor"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    if from_header.is_some() {
        return from_header;
    }
    payload
        .get("target_vendor")
        .or_else(|| payload.get("target_provider"))
        .or_else(|| payload.get("provider"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::resolve_upstream;

    #[test]
    fn resolve_upstream_minimax_global() {
        let r = resolve_upstream(None, Some("minimax:global"));
        let (url, family) = r.expect("get_endpoint(minimax:global) should return Some");
        assert!(
            url.contains("minimax"),
            "base_url should contain minimax: {}",
            url
        );
        assert_eq!(family.as_deref(), Some("minimax"));
    }
}
