use std::collections::HashMap;

use crate::endpoint_context::DriverEndpointContext;

/// Builds the shared part of an outbound upstream header map for a renderer:
/// endpoint `http_header.*` metadata entries plus allowlisted request-metadata
/// forwarding, layered over protocol-specific static headers.
pub(crate) fn base_outbound_headers(
    endpoint: &DriverEndpointContext,
    request_metadata: &HashMap<String, String>,
    static_headers: HashMap<String, String>,
) -> HashMap<String, String> {
    let mut headers = static_headers;

    for (key, value) in &endpoint.metadata {
        let Some(header_name) = key.strip_prefix("http_header.") else {
            continue;
        };
        if !value.is_empty() {
            headers.insert(header_name.to_string(), value.clone());
        }
    }

    forward_metadata_as_http_headers(
        &mut headers,
        request_metadata,
        endpoint.forward_metadata_as_headers.as_deref(),
    );

    headers
}

/// Returns whether a metadata key is blocked from implicit header forwarding.
pub fn is_internal_metadata_key(key: &str) -> bool {
    key.starts_with("unigateway.") || key.starts_with("http_header.")
}

/// Returns whether `key` matches an allowlist entry (exact, case-insensitive, or `prefix*` glob).
pub fn metadata_key_matches_allowlist(key: &str, allowlist: &[String]) -> bool {
    let key_lower = key.to_ascii_lowercase();
    allowlist.iter().any(|pattern| {
        if pattern.contains('*') {
            metadata_glob_match(pattern, key) || metadata_glob_match(pattern, &key_lower)
        } else {
            pattern.eq_ignore_ascii_case(key)
        }
    })
}

fn metadata_glob_match(pattern: &str, key: &str) -> bool {
    let Some(prefix) = pattern.strip_suffix('*') else {
        return pattern.eq_ignore_ascii_case(key);
    };
    if prefix.is_empty() {
        return true;
    }
    key.len() >= prefix.len() && key[..prefix.len()].eq_ignore_ascii_case(prefix)
}

/// Returns true when `value` is safe to use as an HTTP header field value.
pub fn is_valid_http_header_value(value: &str) -> bool {
    !value.is_empty()
        && !value
            .bytes()
            .any(|byte| byte == b'\r' || byte == b'\n' || byte < 0x20)
}

/// Forwards allowlisted request metadata entries into outbound HTTP headers.
pub fn forward_metadata_as_http_headers(
    headers: &mut HashMap<String, String>,
    request_metadata: &HashMap<String, String>,
    allowlist: Option<&[String]>,
) {
    let Some(allowlist) = allowlist.filter(|list| !list.is_empty()) else {
        return;
    };

    for (key, value) in request_metadata {
        if is_internal_metadata_key(key) && !metadata_key_matches_allowlist(key, allowlist) {
            continue;
        }
        if !metadata_key_matches_allowlist(key, allowlist) {
            continue;
        }
        if !is_valid_http_header_value(value) {
            continue;
        }
        headers.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

pub fn merge_forward_allowlists(
    pool: Option<&Vec<String>>,
    endpoint: Option<&Vec<String>>,
) -> Option<Vec<String>> {
    match (pool, endpoint) {
        (None, None) => None,
        (Some(pool), None) => Some(pool.clone()),
        (None, Some(endpoint)) => Some(endpoint.clone()),
        (Some(pool), Some(endpoint)) => {
            let mut merged = pool.clone();
            for item in endpoint {
                if !merged
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(item))
                {
                    merged.push(item.clone());
                }
            }
            Some(merged)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{
        forward_metadata_as_http_headers, is_internal_metadata_key, is_valid_http_header_value,
        merge_forward_allowlists, metadata_key_matches_allowlist,
    };

    #[test]
    fn glob_and_exact_allowlist_matching() {
        let allowlist = vec!["X-Tenant-Id".to_string(), "X-Custom-*".to_string()];
        assert!(metadata_key_matches_allowlist("X-Tenant-Id", &allowlist));
        assert!(metadata_key_matches_allowlist("x-tenant-id", &allowlist));
        assert!(metadata_key_matches_allowlist("X-Custom-Trace", &allowlist));
        assert!(!metadata_key_matches_allowlist("X-Other", &allowlist));
    }

    #[test]
    fn internal_metadata_not_forwarded_without_explicit_allowlist() {
        assert!(is_internal_metadata_key("unigateway.client_protocol"));
        assert!(is_internal_metadata_key("http_header.X-Title"));
    }

    #[test]
    fn forwards_allowlisted_metadata_as_headers() {
        let allowlist = vec!["X-Tenant-Id".to_string()];
        let metadata = HashMap::from([
            ("X-Tenant-Id".to_string(), "tenant-a".to_string()),
            (
                "unigateway.client_protocol".to_string(),
                "openai_chat".to_string(),
            ),
        ]);
        let mut headers = HashMap::new();
        forward_metadata_as_http_headers(&mut headers, &metadata, Some(&allowlist));
        assert_eq!(
            headers.get("X-Tenant-Id").map(String::as_str),
            Some("tenant-a")
        );
        assert!(!headers.contains_key("unigateway.client_protocol"));
    }

    #[test]
    fn rejects_invalid_header_values() {
        assert!(!is_valid_http_header_value(""));
        assert!(!is_valid_http_header_value("bad\nvalue"));
    }

    #[test]
    fn merge_forward_allowlists_deduplicates() {
        let merged = merge_forward_allowlists(
            Some(&vec!["X-A".to_string()]),
            Some(&vec!["X-A".to_string(), "X-B".to_string()]),
        );
        assert_eq!(merged, Some(vec!["X-A".to_string(), "X-B".to_string()]));
    }
}
