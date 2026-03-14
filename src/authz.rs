use std::sync::Arc;

use axum::http::HeaderMap;

use crate::types::AppState;

pub(crate) async fn is_admin_authorized(state: &Arc<AppState>, headers: &HeaderMap) -> bool {
    if !state.config.admin_token.is_empty() {
        let token = headers
            .get("x-admin-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        return token == state.config.admin_token;
    }
    true
}
