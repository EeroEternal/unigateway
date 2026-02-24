pub mod templates;

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
    page("UniGateway - 仪表盘", templates::ADMIN_PAGE)
}

pub fn providers_page() -> String {
    page("UniGateway - 模型管理", templates::PROVIDERS_PAGE)
}

pub fn keys_page() -> String {
    page("UniGateway - API Keys", templates::KEYS_PAGE)
}

pub fn logs_page() -> String {
    page("UniGateway - 请求日志", templates::LOGS_PAGE)
}

pub fn settings_page() -> String {
    page("UniGateway - 设置", templates::SETTINGS_PAGE)
}

pub fn login_error_page() -> String {
    simple_page("登录失败", templates::LOGIN_ERROR_PAGE)
}

pub fn stats_partial(total: i64, openai_count: i64, anthropic_count: i64) -> String {
    templates::STATS_PARTIAL
        .replace("{{total}}", &total.to_string())
        .replace("{{openai_count}}", &openai_count.to_string())
        .replace("{{anthropic_count}}", &anthropic_count.to_string())
}
