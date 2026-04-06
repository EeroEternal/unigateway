use anyhow::Error;
use axum::http::StatusCode;

pub(crate) fn status_for_core_error(error: &Error) -> StatusCode {
    if error.to_string().contains("matches target") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::BAD_GATEWAY
    }
}

pub(crate) fn status_for_legacy_error(error: &Error) -> StatusCode {
    if error.to_string().contains("matches target") {
        StatusCode::BAD_REQUEST
    } else if error.to_string().contains("all providers failed") {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use axum::http::StatusCode;

    use super::{status_for_core_error, status_for_legacy_error};

    #[test]
    fn core_error_status_distinguishes_target_mismatch() {
        assert_eq!(
            status_for_core_error(&anyhow!("no provider matches target 'deepseek'")),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_for_core_error(&anyhow!("upstream request failed")),
            StatusCode::BAD_GATEWAY
        );
    }

    #[test]
    fn legacy_error_status_distinguishes_routing_and_upstream_failures() {
        assert_eq!(
            status_for_legacy_error(&anyhow!("no provider matches target 'deepseek'")),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status_for_legacy_error(&anyhow!("all providers failed, last: boom")),
            StatusCode::BAD_GATEWAY
        );
        assert_eq!(
            status_for_legacy_error(&anyhow!("upstream timed out")),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
