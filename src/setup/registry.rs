use llm_providers::{get_model_for_endpoint, get_providers_data, list_models_for_endpoint};

#[derive(Clone)]
pub(super) struct RegistryProviderOption {
    pub(super) display_name: String,
    pub(super) family_id: String,
    pub(super) provider_type: String,
    pub(super) endpoint_id: String,
    pub(super) default_base_url: String,
    pub(super) model_ids: Vec<String>,
}

fn registry_provider_type(family_id: &str, base_url: &str) -> Option<&'static str> {
    if family_id == "anthropic" {
        return Some("anthropic");
    }
    if base_url.contains("/v1") {
        return Some("openai");
    }
    None
}

pub(super) fn preferred_model_for_endpoint(
    endpoint_id: &str,
    model_ids: &[String],
) -> Option<String> {
    model_ids
        .iter()
        .filter_map(|model_id| {
            get_model_for_endpoint(endpoint_id, model_id).map(|model| {
                (
                    model.supports_tools,
                    model.context_length.unwrap_or(0),
                    model.id.to_string(),
                )
            })
        })
        .max_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)))
        .map(|(_, _, model_id)| model_id)
        .or_else(|| model_ids.first().cloned())
}

pub(super) fn registry_provider_options() -> Vec<RegistryProviderOption> {
    let mut options = Vec::new();

    for (family_id, provider) in get_providers_data().entries() {
        let Some(endpoint) = provider.endpoints.get("global") else {
            continue;
        };
        if endpoint.region != "global" {
            continue;
        }
        let Some(provider_type) = registry_provider_type(family_id, endpoint.base_url) else {
            continue;
        };

        let endpoint_id = format!("{}:global", family_id);
        let model_ids = list_models_for_endpoint(&endpoint_id).unwrap_or_default();
        options.push(RegistryProviderOption {
            display_name: endpoint.label.to_string(),
            family_id: family_id.to_string(),
            provider_type: provider_type.to_string(),
            endpoint_id,
            default_base_url: endpoint.base_url.to_string(),
            model_ids,
        });
    }

    options.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    options.push(RegistryProviderOption {
        display_name: "Other (OpenAI-compatible)".to_string(),
        family_id: "custom".to_string(),
        provider_type: "openai".to_string(),
        endpoint_id: String::new(),
        default_base_url: String::new(),
        model_ids: Vec::new(),
    });
    options
}
