use std::collections::HashMap;

use crate::capabilities::EndpointCapabilities;
use crate::pool::{EndpointId, ModelPolicy, ProviderKind, SecretString};

/// Context allocated dynamically and passed to a driver when performing a request.
///
/// Always available (including conversion-only builds) so request builders such as
/// `protocol::openai::build_chat_request` can be used without linking the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverEndpointContext {
    /// The unique endpoint ID handling the current dispatch
    pub endpoint_id: EndpointId,
    /// The canonical provider kind
    pub provider_kind: ProviderKind,
    /// The base URL mapped to this specific remote
    pub base_url: String,
    /// The authorization secret credential
    pub api_key: SecretString,
    /// Model renaming and fallback maps
    pub model_policy: ModelPolicy,
    /// Resolved endpoint capability declarations used during protocol rendering.
    pub capabilities: EndpointCapabilities,
    /// Arbitrary configuration attributes
    pub metadata: HashMap<String, String>,
    /// Resolved allowlist for forwarding request metadata to outbound HTTP headers.
    pub forward_metadata_as_headers: Option<Vec<String>>,
}

impl DriverEndpointContext {
    /// Resolves the upstream model name for this endpoint: an explicit mapping
    /// wins, then the default model, then the requested name verbatim.
    ///
    /// Callers should pin one fixed endpoint context per render when byte-level
    /// determinism matters: switching endpoints may rewrite the model name and
    /// invalidate upstream prefix caches by design.
    pub fn resolve_model(&self, requested_model: &str) -> String {
        self.model_policy
            .model_mapping
            .get(requested_model)
            .cloned()
            .or_else(|| self.model_policy.default_model.clone())
            .unwrap_or_else(|| requested_model.to_string())
    }

    /// Joins the endpoint base URL with a version-relative API path,
    /// tolerating a trailing slash on the configured base URL.
    pub fn api_url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(base_url: &str, model_policy: ModelPolicy) -> DriverEndpointContext {
        DriverEndpointContext {
            endpoint_id: "ep-1".to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            base_url: base_url.to_string(),
            api_key: SecretString::new("sk-test"),
            model_policy,
            capabilities: EndpointCapabilities::default(),
            metadata: HashMap::new(),
            forward_metadata_as_headers: None,
        }
    }

    #[test]
    fn resolve_model_prefers_mapping_then_default_then_verbatim() {
        let mapped = context(
            "https://upstream.example.com",
            ModelPolicy {
                default_model: Some("fallback-model".to_string()),
                model_mapping: HashMap::from([("alias".to_string(), "mapped-model".to_string())]),
            },
        );
        assert_eq!(mapped.resolve_model("alias"), "mapped-model");
        assert_eq!(mapped.resolve_model("unknown"), "fallback-model");

        let verbatim = context(
            "https://upstream.example.com",
            ModelPolicy {
                default_model: None,
                model_mapping: HashMap::new(),
            },
        );
        assert_eq!(verbatim.resolve_model("requested-model"), "requested-model");
    }

    #[test]
    fn api_url_tolerates_trailing_slash_on_base_url() {
        let with_slash = context("https://api.example.com/v1/", ModelPolicy::default());
        let without_slash = context("https://api.example.com/v1", ModelPolicy::default());
        assert_eq!(
            with_slash.api_url("chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(
            without_slash.api_url("chat/completions"),
            with_slash.api_url("chat/completions")
        );
    }
}
