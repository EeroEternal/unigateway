pub mod templates;
use llm_providers::get_providers_data;

pub fn page(title: &str, body: &str) -> String {
    templates::LAYOUT
        .replace("{{title}}", title)
        .replace("{{body}}", body)
}

pub fn simple_page(title: &str, body: &str) -> String {
    templates::SIMPLE_LAYOUT
        .replace("{{title}}", title)
        .replace("{{body}}", body)
}

pub fn login_page() -> String {
    simple_page("UniGateway Login", templates::LOGIN_PAGE)
}

pub fn admin_page() -> String {
    page("UniGateway - Dashboard", templates::ADMIN_PAGE)
}

fn generate_provider_options() -> String {
    let providers = get_providers_data();
    let mut sorted_providers: Vec<_> = providers.entries().collect();
    sorted_providers.sort_by_key(|(k, _)| *k);

    let mut options = String::new();
    for (id, provider) in sorted_providers {
        let has_global = provider.endpoints.values().any(|ep| ep.region == "global");
        if has_global {
             options.push_str(&format!(r#"<option value="{}">{}</option>"#, id, provider.label));
        }
    }
    options
}

pub fn render_providers_body() -> String {
    let options = generate_provider_options();
    templates::PROVIDERS_PAGE.replace("{{provider_options}}", &options)
}

pub fn providers_page() -> String {
    page("UniGateway - Providers", &render_providers_body())
}

pub fn provider_detail_page(body: &str) -> String {
    page("UniGateway - Provider Detail", body)
}

pub fn keys_page() -> String {
    page("UniGateway - API Keys", templates::KEYS_PAGE)
}

pub fn services_page() -> String {
    page("UniGateway - Services", templates::SERVICES_PAGE)
}

pub fn service_detail_page(body: &str) -> String {
    page("UniGateway - Service Detail", body)
}

pub fn api_key_detail_page(body: &str) -> String {
    page("UniGateway - API Key Detail", body)
}

pub fn logs_page() -> String {
    page("UniGateway - Request Logs", templates::LOGS_PAGE)
}

pub fn settings_page() -> String {
    page("UniGateway - Settings", templates::SETTINGS_PAGE)
}

pub fn login_error_page() -> String {
    simple_page("Login Failed", templates::LOGIN_ERROR_PAGE)
}

pub fn stats_partial(total: i64, api_keys: i64, providers: i64, services: i64) -> String {
    templates::STATS_PARTIAL
        .replace("{{total}}", &total.to_string())
        .replace("{{api_keys}}", &api_keys.to_string())
        .replace("{{providers}}", &providers.to_string())
        .replace("{{services}}", &services.to_string())
}
