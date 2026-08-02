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
}
