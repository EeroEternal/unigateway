use llm_providers::get_endpoint;

/// Normalize a base_url by ensuring it has a trailing slash.
pub fn normalize_base_url(url: &str) -> String {
    let mut s = url.trim().to_string();
    if s.is_empty() {
        return s;
    }
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}

/// Resolves upstream base_url and optional family_id.
///
/// Priority:
/// 1. If `endpoint_id` is provided and recognized by `llm_providers`, use its `base_url`.
/// 2. Otherwise, use `provider_base_url` (if it's not empty).
pub fn resolve_upstream(
    provider_base_url: Option<String>,
    endpoint_id: Option<&str>,
) -> Option<(String, Option<String>)> {
    if let Some(eid) = endpoint_id {
        let eid = eid.trim();
        if !eid.is_empty() {
            if let Some((family_id, endpoint)) = get_endpoint(eid) {
                return Some((
                    normalize_base_url(endpoint.base_url),
                    Some(family_id.to_string()),
                ));
            }
            tracing::debug!(
                "get_endpoint({:?}) returned None, falling back to provider base_url",
                eid
            );
        }
    }

    let url = provider_base_url.as_deref()?.trim();
    if url.is_empty() {
        return None;
    }
    Some((normalize_base_url(url), None))
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
