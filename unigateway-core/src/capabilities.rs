use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::pool::Endpoint;

/// Client or upstream `tool_choice` mode classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceMode {
    Auto,
    None,
    Required,
    Any,
    NamedFunction,
    NamedTool,
}

/// Downgrade target for an unsupported `tool_choice` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceDowngradeTarget {
    Auto,
    None,
    Required,
    Any,
}

/// Provider-scoped tool calling compatibility and downgrade policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallingCapabilities {
    #[serde(default = "ToolCallingCapabilities::default_supported_openai_compatible")]
    pub supported_modes: Vec<ToolChoiceMode>,
    #[serde(default = "ToolCallingCapabilities::default_openai_compatible_downgrade_policy")]
    pub downgrade_policy: HashMap<ToolChoiceMode, ToolChoiceDowngradeTarget>,
    #[serde(default = "default_true")]
    pub streaming_tool_calls: bool,
    /// When true, named tool/function downgrades require a single matching tool declaration.
    #[serde(default = "default_true")]
    pub require_safe_downgrade: bool,
}

fn default_true() -> bool {
    true
}

/// OpenAI HTTP surface compatibility for tools + reasoning combinations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OpenAiApiSurfaceCapabilities {
    /// When `false`, `tools` + `reasoning_effort` must not use Chat Completions (use Responses).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_completions_tools_with_reasoning_effort: Option<bool>,
    /// When `true`, `tools` + `reasoning` are supported on `/v1/responses`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses_tools_with_reasoning: Option<bool>,
    /// When `false`, failed Responses requests must not retry with tools stripped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optional_tools_on_responses_failure_retry: Option<bool>,
}

impl OpenAiApiSurfaceCapabilities {
    /// Built-in defaults for well-known OpenAI model families, merged with endpoint overrides.
    pub fn resolve_for_model(model: &str, endpoint_override: Option<&Self>) -> Self {
        let defaults = Self::defaults_for_model(model);
        let Some(endpoint_override) = endpoint_override else {
            return defaults;
        };

        Self {
            chat_completions_tools_with_reasoning_effort: endpoint_override
                .chat_completions_tools_with_reasoning_effort
                .or(defaults.chat_completions_tools_with_reasoning_effort),
            responses_tools_with_reasoning: endpoint_override
                .responses_tools_with_reasoning
                .or(defaults.responses_tools_with_reasoning),
            optional_tools_on_responses_failure_retry: endpoint_override
                .optional_tools_on_responses_failure_retry
                .or(defaults.optional_tools_on_responses_failure_retry),
        }
    }

    fn defaults_for_model(model: &str) -> Self {
        let normalized = model.to_ascii_lowercase();
        if normalized.contains("gpt-5.5") {
            Self {
                chat_completions_tools_with_reasoning_effort: Some(false),
                responses_tools_with_reasoning: Some(true),
                optional_tools_on_responses_failure_retry: Some(false),
            }
        } else if normalized.contains("gpt-5.4") {
            Self {
                chat_completions_tools_with_reasoning_effort: Some(true),
                responses_tools_with_reasoning: Some(true),
                optional_tools_on_responses_failure_retry: Some(true),
            }
        } else {
            Self {
                chat_completions_tools_with_reasoning_effort: None,
                responses_tools_with_reasoning: None,
                optional_tools_on_responses_failure_retry: Some(true),
            }
        }
    }
}

pub const OPENAI_CHAT_TOOLS_WITH_REASONING_EFFORT_KEY: &str =
    "unigateway.openai.chat_completions.tools_with_reasoning_effort";
pub const OPENAI_RESPONSES_TOOLS_WITH_REASONING_KEY: &str =
    "unigateway.openai.responses.tools_with_reasoning";
pub const OPENAI_RESPONSES_OPTIONAL_TOOLS_RETRY_KEY: &str =
    "unigateway.openai.responses.optional_tools_failure_retry";

/// Endpoint-level capability declarations merged with driver defaults at dispatch time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calling: Option<ToolCallingCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openai_api_surface: Option<OpenAiApiSurfaceCapabilities>,
}

/// Anthropic downstream thinking block rendering policy for OpenAI-compatible upstreams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnthropicThinkingOutputPolicy {
    /// Emit thinking blocks only when upstream supplies structured thinking with a real signature.
    #[default]
    Structured,
    /// Do not synthesize Anthropic thinking blocks for the downstream client.
    OmitThinking,
    /// Emit placeholder signatures for SDK-shape compatibility; strict Anthropic SDKs may still fail.
    PlaceholderThinking,
}

/// Provider-scoped reasoning compatibility declarations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningCapabilities {
    #[serde(default)]
    pub anthropic_thinking_output: AnthropicThinkingOutputPolicy,
}

impl AnthropicThinkingOutputPolicy {
    pub const fn as_metadata_value(self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::OmitThinking => "omit_thinking",
            Self::PlaceholderThinking => "placeholder_thinking",
        }
    }

    pub fn from_metadata_value(value: &str) -> Option<Self> {
        match value {
            "structured" => Some(Self::Structured),
            "omit_thinking" => Some(Self::OmitThinking),
            "placeholder_thinking" => Some(Self::PlaceholderThinking),
            _ => None,
        }
    }
}

impl ReasoningCapabilities {
    pub fn openai_compatible_default() -> Self {
        Self {
            anthropic_thinking_output: AnthropicThinkingOutputPolicy::OmitThinking,
        }
    }

    pub fn anthropic_native_default() -> Self {
        Self {
            anthropic_thinking_output: AnthropicThinkingOutputPolicy::Structured,
        }
    }
}

impl ToolCallingCapabilities {
    fn default_supported_openai_compatible() -> Vec<ToolChoiceMode> {
        Self::openai_compatible_default().supported_modes
    }

    fn default_openai_compatible_downgrade_policy()
    -> HashMap<ToolChoiceMode, ToolChoiceDowngradeTarget> {
        Self::openai_compatible_default().downgrade_policy
    }

    /// Conservative default for OpenAI-compatible upstreams (e.g. DeepSeek-style APIs).
    pub fn openai_compatible_default() -> Self {
        Self {
            supported_modes: vec![ToolChoiceMode::Auto, ToolChoiceMode::None],
            downgrade_policy: HashMap::from([
                (
                    ToolChoiceMode::NamedFunction,
                    ToolChoiceDowngradeTarget::Auto,
                ),
                (ToolChoiceMode::Required, ToolChoiceDowngradeTarget::Auto),
            ]),
            streaming_tool_calls: true,
            require_safe_downgrade: true,
        }
    }

    /// Default for native Anthropic upstreams.
    pub fn anthropic_native_default() -> Self {
        Self {
            supported_modes: vec![
                ToolChoiceMode::Auto,
                ToolChoiceMode::None,
                ToolChoiceMode::Any,
                ToolChoiceMode::Required,
                ToolChoiceMode::NamedTool,
            ],
            downgrade_policy: HashMap::new(),
            streaming_tool_calls: true,
            require_safe_downgrade: true,
        }
    }

    /// Upstreams that reject named function choice but accept `required` (memtensor / taotoken).
    pub fn memtensor_style() -> Self {
        Self {
            supported_modes: vec![
                ToolChoiceMode::Auto,
                ToolChoiceMode::None,
                ToolChoiceMode::Required,
            ],
            downgrade_policy: HashMap::from([(
                ToolChoiceMode::NamedFunction,
                ToolChoiceDowngradeTarget::Required,
            )]),
            streaming_tool_calls: true,
            require_safe_downgrade: true,
        }
    }
}

impl EndpointCapabilities {
    /// Returns explicit endpoint capabilities or built-in driver defaults.
    pub fn resolve_for_endpoint(endpoint: &Endpoint) -> Self {
        let tool_calling = endpoint
            .capabilities
            .tool_calling
            .clone()
            .or_else(|| Some(default_tool_calling_for_driver(&endpoint.driver_id)));
        let reasoning = endpoint
            .capabilities
            .reasoning
            .clone()
            .or_else(|| Some(default_reasoning_for_driver(&endpoint.driver_id)));

        Self {
            tool_calling,
            reasoning,
            openai_api_surface: endpoint.capabilities.openai_api_surface.clone(),
        }
    }

    pub fn tool_calling(&self) -> ToolCallingCapabilities {
        self.tool_calling
            .clone()
            .unwrap_or_else(ToolCallingCapabilities::openai_compatible_default)
    }

    pub fn reasoning(&self) -> ReasoningCapabilities {
        self.reasoning
            .clone()
            .unwrap_or_else(ReasoningCapabilities::openai_compatible_default)
    }

    pub fn openai_api_surface(&self) -> Option<&OpenAiApiSurfaceCapabilities> {
        self.openai_api_surface.as_ref()
    }

    pub fn inject_metadata(&self, metadata: &mut std::collections::HashMap<String, String>) {
        metadata.insert(
            crate::request::ANTHROPIC_THINKING_OUTPUT_KEY.to_string(),
            self.reasoning()
                .anthropic_thinking_output
                .as_metadata_value()
                .to_string(),
        );
        if let Some(surface) = self.openai_api_surface.as_ref() {
            inject_openai_api_surface_metadata(metadata, surface);
        }
    }
}

fn inject_openai_api_surface_metadata(
    metadata: &mut std::collections::HashMap<String, String>,
    surface: &OpenAiApiSurfaceCapabilities,
) {
    if let Some(value) = surface.chat_completions_tools_with_reasoning_effort {
        metadata.insert(
            OPENAI_CHAT_TOOLS_WITH_REASONING_EFFORT_KEY.to_string(),
            value.to_string(),
        );
    }
    if let Some(value) = surface.responses_tools_with_reasoning {
        metadata.insert(
            OPENAI_RESPONSES_TOOLS_WITH_REASONING_KEY.to_string(),
            value.to_string(),
        );
    }
    if let Some(value) = surface.optional_tools_on_responses_failure_retry {
        metadata.insert(
            OPENAI_RESPONSES_OPTIONAL_TOOLS_RETRY_KEY.to_string(),
            value.to_string(),
        );
    }
}

fn default_tool_calling_for_driver(driver_id: &str) -> ToolCallingCapabilities {
    match driver_id {
        "anthropic" => ToolCallingCapabilities::anthropic_native_default(),
        _ => ToolCallingCapabilities::openai_compatible_default(),
    }
}

fn default_reasoning_for_driver(driver_id: &str) -> ReasoningCapabilities {
    match driver_id {
        "anthropic" => ReasoningCapabilities::anthropic_native_default(),
        _ => ReasoningCapabilities::openai_compatible_default(),
    }
}
