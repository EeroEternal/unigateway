use anyhow::Error;
use http::StatusCode;

pub fn status_for_core_error(error: &Error) -> StatusCode {
    if error.to_string().contains("matches target") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::BAD_GATEWAY
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;
    use http::StatusCode;

    use super::status_for_core_error;

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
}
