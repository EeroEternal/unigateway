#[path = "admin/api.rs"]
mod api;
#[path = "admin/authz.rs"]
mod authz;
#[path = "admin/dto.rs"]
mod dto;
#[path = "admin/mutations.rs"]
mod mutations;
#[path = "admin/pages.rs"]
mod pages;
#[path = "admin/partials.rs"]
mod partials;
#[path = "admin/queries.rs"]
mod queries;
#[path = "admin/render.rs"]
mod render;
#[path = "admin/shell.rs"]
mod shell;

pub(crate) use api::{
    api_bind_provider, api_create_api_key, api_create_provider, api_create_service,
    api_list_api_keys, api_list_providers, api_list_services, health, metrics, models,
};
pub(crate) use pages::{
    admin_api_key_detail_page, admin_api_keys_page, admin_dashboard, admin_logs_page,
    admin_page, admin_provider_detail_page, admin_providers, admin_service_detail_page,
    admin_services_page, admin_settings_page, home,
};
pub(crate) use partials::{
    admin_api_keys_delete, admin_api_keys_list_partial, admin_create_api_key_partial,
    admin_create_provider_partial, admin_logs_list_partial, admin_providers_delete,
    admin_providers_list_partial, admin_services_delete, admin_services_list_partial,
    admin_stats_partial,
};
