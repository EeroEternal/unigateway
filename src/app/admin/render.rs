use super::dto::{
    ApiKeyListRow, ProviderListRow, ProviderOptionRow, ServiceListRow, ServiceSummaryRow,
    ServiceDetailProviderRow, ServiceTokenRow, LogRow,
};
use crate::ui;

pub(crate) fn render_provider_detail_service_rows(services: Vec<ServiceSummaryRow>) -> String {
    let mut rows = String::new();
    for service in services {
        rows.push_str(&format!(
            "<tr>
              <td class='py-4 px-6 border-b border-slate-100'>
                <button onclick='openServiceDetail(&quot;{}&quot;)' class='font-semibold text-slate-800 hover:text-teal-800 transition-colors'>{}</button>
              </td>
              <td class='py-4 px-6 border-b border-slate-100'><code class='text-[12px] font-mono text-slate-600 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code></td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm font-semibold text-slate-500'>{}</td>
            </tr>",
            service.id, service.name, service.id, service.created_at
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='3' class='py-10 text-center text-slate-400 font-semibold'>No bound services</td></tr>");
    }
    rows
}

pub(crate) fn render_service_detail_provider_rows(providers: Vec<ServiceDetailProviderRow>) -> String {
    let mut rows = String::new();
    for provider in providers {
        rows.push_str(&format!(
            "<tr>
              <td class='py-4 px-6 border-b border-slate-100 font-semibold text-slate-800'>{}</td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm text-slate-500'>{}</td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm text-slate-500'>{}</td>
            </tr>",
            provider.name, provider.provider_type, provider.endpoint_id
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='3' class='py-10 text-center text-slate-400 font-semibold'>No providers bound</td></tr>");
    }
    rows
}

pub(crate) fn render_service_detail_api_key_rows(api_keys: Vec<ServiceTokenRow>) -> String {
    let mut rows = String::new();
    for api_key in api_keys {
        rows.push_str(&format!(
            "<tr>
              <td class='py-4 px-6 border-b border-slate-100'><button onclick='openApiKeyDetail(&quot;{}&quot;)' class='font-semibold text-slate-800 hover:text-teal-800 transition-colors'>{}</button></td>
              <td class='py-4 px-6 border-b border-slate-100'><code class='text-[12px] font-mono text-slate-600'>{}</code></td>
              <td class='py-4 px-6 border-b border-slate-100 text-sm text-slate-500'>{}</td>
            </tr>",
            api_key.key,
            api_key.name.unwrap_or_default(),
            api_key.key,
            api_key.created_at
        ));
    }
    if rows.is_empty() {
        rows.push_str("<tr><td colspan='3' class='py-10 text-center text-slate-400 font-semibold'>No API keys</td></tr>");
    }
    rows
}

pub(crate) fn render_provider_options(providers: Vec<ProviderOptionRow>) -> String {
    let mut options = String::new();
    for ProviderOptionRow { id, name, provider_type } in providers {
        options.push_str(&format!(
            r#"<label class="label cursor-pointer justify-start gap-3 rounded-lg border border-slate-200 bg-white px-3 py-2 hover:border-brand/40">
<input type="checkbox" name="provider_ids[]" value="{}" class="checkbox checkbox-sm checkbox-primary" />
<span class="text-sm font-semibold text-slate-700">{}</span>
<span class="badge badge-ghost text-[10px] uppercase tracking-wider">{}</span>
</label>"#,
            id, name, provider_type
        ));
    }
    if options.is_empty() {
        options.push_str(r#"<div class="text-xs text-slate-400 px-2 py-3">No providers available</div>"#);
    }
    options
}

pub(crate) fn render_provider_list_rows(providers: Vec<ProviderListRow>) -> String {
    let mut rows_html = String::new();
    for provider in providers {
        let first_char = provider.name.chars().next().unwrap_or('?');
        let _ = &provider.service_ids;
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                  <div class='w-8 h-8 bg-slate-100 rounded-lg flex items-center justify-center text-slate-400 font-bold group-hover:bg-brand group-hover:text-white transition-all uppercase text-[11px]'>
                      {}
                  </div>
                  <button onclick='openProviderDetail({})' class='font-bold text-slate-700 text-sm tracking-tight hover:text-teal-800 transition-colors'>{}</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1.5 rounded-md text-[10px] uppercase tracking-widest h-auto shadow-none'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <code class='text-[12px] font-mono text-slate-600 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code>
                  <code class='text-[11px] font-mono text-slate-500 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md'>{}</code>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <button onclick='openProviderDetail({})' class='w-fit badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1.5 rounded-md text-[10px] uppercase tracking-widest h-auto shadow-none hover:border-brand/30 hover:text-brand transition-colors'>{} linked</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button
                  hx-delete='/admin/providers/{}'
                  hx-target='#providers-list'
                  hx-confirm='Are you sure you want to remove this provider?'
                  class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'
                >
                  Remove
                </button>
              </td>
            </tr>",
            first_char,
            provider.id,
            provider.name,
            provider.provider_type,
            provider.endpoint_id.unwrap_or_default(),
            provider.base_url.unwrap_or_default(),
            provider.id,
            provider.service_count,
            provider.id
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='5' class='text-center py-20 text-slate-300 font-bold'>No model providers found</td></tr>");
    }
    rows_html
}

pub(crate) fn render_provider_detail_body(
    provider_name: &str,
    provider_type: &str,
    endpoint_id: Option<&str>,
    base_url: Option<&str>,
    service_rows: &str,
) -> String {
    ui::templates::PROVIDER_DETAIL_PAGE
        .replace("{{provider_name}}", provider_name)
        .replace("{{provider_type}}", provider_type)
        .replace("{{endpoint_id}}", endpoint_id.unwrap_or("-"))
        .replace("{{base_url}}", base_url.unwrap_or("-"))
        .replace("{{service_rows}}", service_rows)
}

pub(crate) fn render_service_detail_body(
    service_name: &str,
    service_id: &str,
    created_at: &str,
    provider_rows: &str,
    api_key_rows: &str,
) -> String {
    ui::templates::SERVICE_DETAIL_PAGE
        .replace("{{service_name}}", service_name)
        .replace("{{service_id}}", service_id)
        .replace("{{created_at}}", created_at)
        .replace("{{provider_rows}}", provider_rows)
        .replace("{{api_key_rows}}", api_key_rows)
}

pub(crate) fn render_api_key_detail_body(
    api_key_name: &str,
    api_key_value: &str,
    created_at: &str,
    service_id: &str,
    service_name: &str,
) -> String {
    ui::templates::API_KEY_DETAIL_PAGE
        .replace("{{api_key_name}}", api_key_name)
        .replace("{{api_key_value}}", api_key_value)
        .replace("{{created_at}}", created_at)
        .replace("{{service_id}}", service_id)
        .replace("{{service_name}}", service_name)
}

pub(crate) fn render_api_key_list_rows(keys: Vec<ApiKeyListRow>) -> String {
    let mut rows_html = String::new();
    for row in keys {
        let display_name = row.name.unwrap_or_default();
        let display_service_name = row.service_name.unwrap_or_else(|| {
            if row.service_id == "default" {
                "Default Service".to_string()
            } else {
                row.service_id.clone()
            }
        });
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                    <div class='w-8 h-8 rounded-lg bg-slate-50 border border-slate-100 flex items-center justify-center text-slate-400 font-bold text-xs transition-all group-hover:bg-brand/10 group-hover:border-brand/20 group-hover:text-brand'>
                      {}
                    </div>
                    <div class='flex flex-col'>
                      <button onclick='openApiKeyDetail(&quot;{}&quot;)' class='w-fit text-left font-bold text-slate-800 text-sm tracking-tight hover:text-teal-800 transition-colors'>{}</button>
                    </div>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                    <code class='text-[13px] font-mono font-bold text-slate-700 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight'>{}</code>
                    <button onclick='copyApiKey(&quot;{}&quot;)' class='btn btn-ghost btn-xs h-8 min-h-0 rounded-lg border border-slate-200 bg-white px-3 font-bold text-slate-600 hover:bg-slate-50'>Copy</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-emerald-50 border-emerald-100 text-emerald-600 font-bold px-2.5 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none'>Active</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col'>
                  <button
                    onclick='openServiceDetail(&quot;{}&quot;)'
                    class='w-fit text-[12px] font-bold text-brand hover:text-teal-800 transition-colors tracking-tight'
                  >
                    {}
                  </button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-400 uppercase tracking-widest'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button
                  onclick=\"deleteApiKey('{}')\"
                  class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'
                >
                  Remove
                </button>
              </td>
            </tr>",
            display_name.chars().next().unwrap_or('K').to_ascii_uppercase(),
            row.key,
            display_name,
            row.key,
            row.key,
            row.service_id,
            display_service_name,
            row.created_at,
            row.key
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='6' class='text-center py-20 text-slate-300 font-bold'>No API keys found</td></tr>");
    }
    rows_html
}

pub(crate) fn render_service_list_rows(services: Vec<ServiceListRow>) -> String {
    let mut rows_html = String::new();
    for row in services {
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex items-center gap-3'>
                  <div class='w-8 h-8 rounded-lg bg-slate-50 border border-slate-100 flex items-center justify-center text-slate-400 font-bold text-xs transition-all group-hover:bg-brand/10 group-hover:border-brand/20 group-hover:text-brand'>
                    {}
                  </div>
                  <button onclick='openServiceDetail(&quot;{}&quot;)' class='w-fit text-left font-bold text-slate-800 text-sm tracking-tight hover:text-teal-800 transition-colors'>{}</button>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <code class='text-[11px] font-mono font-bold text-slate-600 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight w-fit'>{}</code>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none w-fit'>{} linked</span>
                  <code class='text-[11px] font-mono text-slate-500 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 tracking-tight w-fit'>{}</code>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <div class='flex flex-col gap-1'>
                  <span class='badge bg-slate-50 border-slate-200 text-slate-500 font-bold px-2.5 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none w-fit'>{} key(s)</span>
                  <div class='text-[11px] text-slate-500 font-medium leading-5 bg-slate-50 px-2.5 py-1 rounded-lg border border-slate-100 w-fit'>{}</div>
                </div>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-400 uppercase tracking-widest'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100 text-right'>
                <button onclick='deleteService(&quot;{}&quot;, {})' class='text-rose-500 hover:text-rose-700 font-bold text-xs transition-colors'>Remove</button>
              </td>
            </tr>",
            row.name.chars().next().unwrap_or('S').to_ascii_uppercase(),
            row.id,
            row.name,
            row.id,
            row.provider_count,
            row.provider_names.unwrap_or_else(|| "No providers bound".to_string()),
            row.token_count,
            row.token_names.unwrap_or_else(|| "No API keys".to_string()),
            row.created_at,
            row.id,
            row.token_count,
        ));
    }

    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='6' class='text-center py-20 text-slate-300 font-bold'>No services found</td></tr>");
    }
    rows_html
}

pub(crate) fn render_log_rows(logs: Vec<LogRow>) -> String {
    let mut rows_html = String::new();
    for row in logs {
        let status_val = row.status_code;
        let status_class = if status_val < 300 {
            "bg-emerald-50 text-emerald-600 border-emerald-100"
        } else {
            "bg-rose-50 text-rose-600 border-rose-100"
        };
        rows_html.push_str(&format!(
            "<tr class='group hover:bg-slate-50 transition-colors'>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-400 uppercase tracking-widest'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <code class='text-[12px] font-mono font-bold text-slate-700 bg-slate-50 border border-slate-100 px-2 py-1 rounded-md tracking-tight'>{}</code>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge {} border font-bold px-2 py-1 rounded-md text-[10px] h-auto shadow-none'>{}</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='text-[11px] font-bold text-slate-500'>{}ms</span>
              </td>
              <td class='py-4 px-8 border-b border-slate-100'>
                <span class='badge bg-slate-50 border-slate-200 text-slate-400 font-bold px-2 py-1 rounded-md text-[10px] uppercase tracking-wider h-auto shadow-none'>{}</span>
              </td>
            </tr>",
            row.created_at,
            row.endpoint,
            status_class,
            status_val,
            row.latency_ms,
            row.provider
        ));
    }
    if rows_html.is_empty() {
        rows_html.push_str("<tr><td colspan='5' class='text-center py-20 text-slate-300 font-bold'>No logs found</td></tr>");
    }
    rows_html
}
