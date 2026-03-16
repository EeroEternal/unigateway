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

pub(super) enum SetupFlow<T> {
    Next(T),
    Back,
}

pub(super) fn resolve_provider_setup(
    labels: ProviderPromptLabels<'_>,
    input: ProviderSetupInput,
) -> SetupFlow<ProviderSetup> {
    if input.provider_type.is_some() && input.endpoint_id.is_some() && input.api_key.is_some() {
        let endpoint_id = input.endpoint_id.unwrap();
        let default_model = input.default_model.or_else(|| {
            let model_ids = list_models_for_endpoint(&endpoint_id).unwrap_or_default();
            preferred_model_for_endpoint(&endpoint_id, &model_ids)
        });
        return SetupFlow::Next(ProviderSetup {
            name: input.provider_type.clone().unwrap(),
            provider_type: input.provider_type.unwrap(),
            endpoint_id,
            default_model,
            base_url: input.base_url,
            api_key: input.api_key.unwrap(),
        });
    }

    let theme = ColorfulTheme::default();
    let registry_options = registry_provider_options();
    let display_names: Vec<&str> = registry_options
        .iter()
        .map(|provider| provider.display_name.as_str())
        .collect();

    let mut step = 0;
    let mut selected_index = input
        .provider_type
        .as_deref()
        .and_then(|p_type| {
            registry_options.iter().position(|p| {
                p.family_id == p_type
                    || p.provider_type == p_type
                    || p.endpoint_id == p_type
                    || display_names
                        .iter()
                        .any(|name| name.to_lowercase() == p_type)
            })
        })
        .unwrap_or(0);

    let mut default_model: Option<String> = input.default_model.clone();
    let mut base_url: Option<String> = input.base_url.clone();
    let mut api_key: Option<String> = input.api_key.clone();

    loop {
        match step {
            0 => {
                if input.provider_type.is_some() {
                    step = 1;
                    continue;
                }
                match Select::with_theme(&theme)
                    .with_prompt(labels.provider)
                    .items(&display_names)
                    .default(selected_index)
                    .interact_opt()
                    .unwrap()
                {
                    Some(index) => {
                        selected_index = index;
                        step = 1;
                    }
                    None => return SetupFlow::Back,
                }
            }
            1 => {
                if input.default_model.is_some() {
                    step = 2;
                    continue;
                }
                let provider = &registry_options[selected_index];
                let endpoint_id = input
                    .endpoint_id
                    .as_deref()
                    .unwrap_or(&provider.endpoint_id);
                let available_models = if endpoint_id.is_empty() {
                    provider.model_ids.clone()
                } else {
                    list_models_for_endpoint(endpoint_id)
                        .unwrap_or_else(|| provider.model_ids.clone())
                };
                let preferred_model = preferred_model_for_endpoint(endpoint_id, &available_models)
                    .or_else(|| available_models.first().cloned())
                    .unwrap_or_default();

                if !available_models.is_empty() {
                    let default_idx = available_models
                        .iter()
                        .position(|m| m == &preferred_model)
                        .unwrap_or(0);
                    match Select::with_theme(&theme)
                        .with_prompt(labels.model)
                        .items(&available_models)
                        .default(default_idx)
                        .interact_opt()
                        .unwrap()
                    {
                        Some(idx) => {
                            default_model = Some(available_models[idx].clone());
                            step = 2;
                        }
                        None => {
                            if input.provider_type.is_some() {
                                return SetupFlow::Back;
                            }
                            step = 0;
                        }
                    }
                } else {
                    match Input::<String>::with_theme(&theme)
                        .with_prompt(labels.model)
                        .default(preferred_model)
                        .interact_text()
                    {
                        Ok(val) => {
                            default_model = Some(val);
                            step = 2;
                        }
                        Err(_) => {
                            if input.provider_type.is_some() {
                                return SetupFlow::Back;
                            }
                            step = 0;
                        }
                    }
                }
            }
            2 => {
                if input.base_url.is_some() {
                    step = 3;
                    continue;
                }
                let provider = &registry_options[selected_index];
                if !provider.default_base_url.is_empty() {
                    match Input::<String>::with_theme(&theme)
                        .with_prompt(labels.base_url)
                        .default(provider.default_base_url.clone())
                        .interact_text()
                    {
                        Ok(val) => {
                            base_url = if val == provider.default_base_url {
                                None
                            } else {
                                Some(val)
                            };
                            step = 3;
                        }
                        Err(_) => {
                            if input.default_model.is_some() {
                                return SetupFlow::Back;
                            }
                            step = 1;
                        }
                    }
                } else {
                    match Input::<String>::with_theme(&theme)
                        .with_prompt(labels.base_url)
                        .interact_text()
                    {
                        Ok(val) => {
                            base_url = Some(val);
                            step = 3;
                        }
                        Err(_) => {
                            if input.default_model.is_some() {
                                return SetupFlow::Back;
                            }
                            step = 1;
                        }
                    }
                }
            }
            3 => {
                if input.api_key.is_some() {
                    step = 4;
                    continue;
                }
                match Password::with_theme(&theme)
                    .with_prompt(labels.api_key)
                    .interact()
                {
                    Ok(val) => {
                        api_key = Some(val);
                        step = 4;
                    }
                    Err(_) => {
                        if input.base_url.is_some() {
                            return SetupFlow::Back;
                        }
                        step = 2;
                    }
                }
            }
            4 => {
                let provider = &registry_options[selected_index];
                let endpoint_id = input
                    .endpoint_id
                    .unwrap_or_else(|| provider.endpoint_id.clone());
                return SetupFlow::Next(ProviderSetup {
                    name: if provider.family_id == "custom" {
                        provider.provider_type.clone()
                    } else {
                        provider.family_id.clone()
                    },
                    provider_type: provider.provider_type.clone(),
                    endpoint_id,
                    default_model,
                    base_url,
                    api_key: api_key.unwrap(),
                });
            }
            _ => unreachable!("setup step out of range"),
        }
    }
}
