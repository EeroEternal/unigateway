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

/// Endpoint-level capability declarations merged with driver defaults at dispatch time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calling: Option<ToolCallingCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningCapabilities>,
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

    pub fn inject_metadata(&self, metadata: &mut std::collections::HashMap<String, String>) {
        metadata.insert(
            crate::request::ANTHROPIC_THINKING_OUTPUT_KEY.to_string(),
            self.reasoning()
                .anthropic_thinking_output
                .as_metadata_value()
                .to_string(),
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
