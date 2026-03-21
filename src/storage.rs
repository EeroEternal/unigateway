use serde_json::Value;

pub(crate) fn map_model_name(model_mapping: Option<&str>, requested_model: &str) -> Option<String> {
    let raw_mapping = model_mapping?;

    if let Ok(value) = serde_json::from_str::<Value>(raw_mapping) {
        if let Some(mapped) = value.get(requested_model).and_then(Value::as_str) {
            return Some(mapped.to_string());
        }
        if let Some(default) = value.get("default").and_then(Value::as_str) {
            return Some(default.to_string());
        }
    }

    if !raw_mapping.trim().is_empty() && !raw_mapping.trim().starts_with('{') {
        return Some(raw_mapping.trim().to_string());
    }

    None
}
