use dialoguer::{Input, Password, Select, theme::ColorfulTheme};
use llm_providers::list_models_for_endpoint;

use super::registry::{preferred_model_for_endpoint, registry_provider_options};

pub(super) struct ProviderPromptLabels<'a> {
    pub(super) provider: &'a str,
    pub(super) model: &'a str,
    pub(super) base_url: &'a str,
    pub(super) api_key: &'a str,
}

pub(super) struct ProviderSetupInput {
    pub(super) provider_type: Option<String>,
    pub(super) endpoint_id: Option<String>,
    pub(super) default_model: Option<String>,
    pub(super) base_url: Option<String>,
    pub(super) api_key: Option<String>,
}

pub(super) struct ProviderSetup {
    pub(super) name: String,
    pub(super) provider_type: String,
    pub(super) endpoint_id: String,
    pub(super) default_model: Option<String>,
    pub(super) base_url: Option<String>,
    pub(super) api_key: String,
}

pub(super) fn resolve_provider_setup(
    labels: ProviderPromptLabels<'_>,
    input: ProviderSetupInput,
) -> ProviderSetup {
    if input.provider_type.is_none() || input.endpoint_id.is_none() || input.api_key.is_none() {
        let theme = ColorfulTheme::default();
        let registry_options = registry_provider_options();
        let display_names: Vec<&str> = registry_options
            .iter()
            .map(|provider| provider.display_name.as_str())
            .collect();
        let selected = input.provider_type.as_deref().and_then(|provider_type| {
            registry_options.iter().position(|provider| {
                provider.family_id == provider_type
                    || provider.provider_type == provider_type
                    || provider.endpoint_id == provider_type
                    || display_names
                        .iter()
                        .any(|name| name.to_lowercase() == provider_type)
            })
        });

        let index = if let Some(selected) = selected {
            selected
        } else if input.provider_type.is_none() {
            Select::with_theme(&theme)
                .with_prompt(labels.provider)
                .items(&display_names)
                .default(0)
                .interact()
                .unwrap()
        } else {
            registry_options.len() - 1
        };

        let provider = &registry_options[index];
        let endpoint_id = input
            .endpoint_id
            .unwrap_or_else(|| provider.endpoint_id.clone());

        let available_models = if endpoint_id.is_empty() {
            provider.model_ids.clone()
        } else {
            list_models_for_endpoint(&endpoint_id).unwrap_or_else(|| provider.model_ids.clone())
        };
        let preferred_model = preferred_model_for_endpoint(&endpoint_id, &available_models)
            .or_else(|| available_models.first().cloned())
            .unwrap_or_default();

        let default_model = input.default_model.or_else(|| {
            if !available_models.is_empty() {
                let default_index = available_models
                    .iter()
                    .position(|model_id| model_id == &preferred_model)
                    .unwrap_or(0);
                Select::with_theme(&theme)
                    .with_prompt(labels.model)
                    .items(&available_models)
                    .default(default_index)
                    .interact()
                    .map(|index| Some(available_models[index].clone()))
                    .unwrap()
            } else if !preferred_model.is_empty() {
                Input::with_theme(&theme)
                    .with_prompt(labels.model)
                    .default(preferred_model)
                    .interact_text()
                    .ok()
            } else {
                Input::with_theme(&theme)
                    .with_prompt(labels.model)
                    .interact_text()
                    .ok()
            }
        });

        let base_url = input.base_url.or_else(|| {
            if !provider.default_base_url.is_empty() {
                let url: String = Input::with_theme(&theme)
                    .with_prompt(labels.base_url)
                    .default(provider.default_base_url.clone())
                    .interact_text()
                    .unwrap();
                if url == provider.default_base_url {
                    None
                } else {
                    Some(url)
                }
            } else {
                let url: String = Input::with_theme(&theme)
                    .with_prompt(labels.base_url)
                    .interact_text()
                    .unwrap();
                Some(url)
            }
        });

        let api_key = input.api_key.unwrap_or_else(|| {
            Password::with_theme(&theme)
                .with_prompt(labels.api_key)
                .interact()
                .unwrap()
        });

        ProviderSetup {
            name: if provider.family_id == "custom" {
                provider.provider_type.clone()
            } else {
                provider.family_id.clone()
            },
            provider_type: provider.provider_type.clone(),
            endpoint_id,
            default_model,
            base_url,
            api_key,
        }
    } else if let (Some(provider_type), Some(endpoint_id), Some(api_key)) =
        (input.provider_type, input.endpoint_id, input.api_key)
    {
        let default_model = input.default_model.or_else(|| {
            let model_ids = list_models_for_endpoint(&endpoint_id).unwrap_or_default();
            preferred_model_for_endpoint(&endpoint_id, &model_ids)
        });
        ProviderSetup {
            name: provider_type.clone(),
            provider_type,
            endpoint_id,
            default_model,
            base_url: input.base_url,
            api_key,
        }
    } else {
        unreachable!("provider setup missing required fields")
    }
}
