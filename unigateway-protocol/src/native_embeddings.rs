//! Provider-native multimodal embeddings shapes (protocol translation).
//!
//! Some embedding models are only served by provider-native services whose
//! request/response shapes differ from the OpenAI `/v1/embeddings` surface —
//! Aliyun DashScope multimodal-embedding and Volcengine Ark
//! `/embeddings/multimodal`. These pure functions translate between the
//! neutral [`ProxyMultimodalEmbeddingsRequest`] / OpenAI response shape and
//! those native shapes. HTTP transport stays with the caller.

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use unigateway_core::{EmbeddingsInputItem, ProxyMultimodalEmbeddingsRequest};

fn reject_base64(request: &ProxyMultimodalEmbeddingsRequest) -> Result<()> {
    if request.encoding_format.as_deref() == Some("base64") {
        return Err(anyhow!(
            "encoding_format=base64 is not supported for native multimodal embedding models"
        ));
    }
    Ok(())
}

/// Build the DashScope native multimodal-embedding request body
/// (`POST /api/v1/services/embeddings/multimodal-embedding/multimodal-embedding`).
///
/// `dimensions` maps to `parameters.dimension`. `encoding_format=base64` is
/// rejected: the native service has no equivalent.
pub fn build_dashscope_multimodal_embeddings_request(
    request: &ProxyMultimodalEmbeddingsRequest,
) -> Result<Value> {
    reject_base64(request)?;
    let mut contents = Vec::with_capacity(request.input.len());
    for item in &request.input {
        contents.push(match item {
            EmbeddingsInputItem::Text { text } => json!({ "text": text }),
            EmbeddingsInputItem::ImageUrl { image_url } => json!({ "image": image_url.url }),
        });
    }
    let mut native = json!({
        "model": request.model,
        "input": { "contents": contents },
    });
    if let Some(dimensions) = request.dimensions {
        native["parameters"] = json!({ "dimension": dimensions });
    }
    Ok(native)
}

/// Build the Volcengine Ark multimodal-embedding request body
/// (`POST {ark}/embeddings/multimodal`).
///
/// Input items carry a `type` tag. `dimensions` is rejected because the
/// native service answers `InvalidParameter` for it; `encoding_format=base64`
/// is rejected as above.
pub fn build_volcengine_multimodal_embeddings_request(
    request: &ProxyMultimodalEmbeddingsRequest,
) -> Result<Value> {
    reject_base64(request)?;
    if request.dimensions.is_some() {
        return Err(anyhow!(
            "dimensions is not supported for Volcengine multimodal embeddings"
        ));
    }
    let mut input = Vec::with_capacity(request.input.len());
    for item in &request.input {
        input.push(match item {
            EmbeddingsInputItem::Text { text } => json!({ "type": "text", "text": text }),
            EmbeddingsInputItem::ImageUrl { image_url } => json!({
                "type": "image_url",
                "image_url": { "url": image_url.url },
            }),
        });
    }
    Ok(json!({ "model": request.model, "input": input }))
}

/// Convert a native DashScope embedding response to the OpenAI
/// `/v1/embeddings` response shape. Handles both `index` (multimodal) and
/// `text_index` (text-embedding native) output flavors.
pub fn dashscope_multimodal_embeddings_response_to_openai(raw: &str, model: &str) -> Result<Value> {
    let parsed: Value =
        serde_json::from_str(raw).map_err(|e| anyhow!("invalid DashScope response: {e}"))?;
    let embeddings = parsed
        .pointer("/output/embeddings")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("DashScope response missing output.embeddings"))?;
    let data: Vec<Value> = embeddings
        .iter()
        .enumerate()
        .map(|(position, entry)| {
            let index = entry
                .get("index")
                .and_then(Value::as_u64)
                .or_else(|| entry.get("text_index").and_then(Value::as_u64))
                .unwrap_or(position as u64);
            json!({
                "object": "embedding",
                "embedding": entry.get("embedding").cloned().unwrap_or(Value::Null),
                "index": index,
            })
        })
        .collect();
    let total_tokens = parsed
        .pointer("/usage/total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Ok(json!({
        "object": "list",
        "data": data,
        "model": model,
        "usage": { "prompt_tokens": total_tokens, "total_tokens": total_tokens },
    }))
}

/// Convert a Volcengine Ark multimodal-embedding response to the OpenAI
/// `/v1/embeddings` response shape.
///
/// The native `data` member is a single object (one combined vector per
/// request), not the OpenAI array of per-input entries.
pub fn volcengine_multimodal_embeddings_response_to_openai(
    raw: &str,
    model: &str,
) -> Result<Value> {
    let parsed: Value =
        serde_json::from_str(raw).map_err(|e| anyhow!("invalid Volcengine response: {e}"))?;
    let entry = parsed
        .get("data")
        .filter(|value| value.is_object())
        .ok_or_else(|| anyhow!("Volcengine response missing data object"))?;
    let total_tokens = parsed
        .pointer("/usage/total_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Ok(json!({
        "object": "list",
        "data": [{
            "object": "embedding",
            "embedding": entry.get("embedding").cloned().unwrap_or(Value::Null),
            "index": 0,
        }],
        "model": model,
        "usage": { "prompt_tokens": total_tokens, "total_tokens": total_tokens },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_with(input: Vec<EmbeddingsInputItem>) -> ProxyMultimodalEmbeddingsRequest {
        ProxyMultimodalEmbeddingsRequest {
            model: "m".to_string(),
            input,
            encoding_format: None,
            dimensions: None,
            metadata: Default::default(),
        }
    }

    #[test]
    fn builds_dashscope_contents_shape() {
        let mut request = request_with(vec![
            EmbeddingsInputItem::Text {
                text: "一张猫的照片".to_string(),
            },
            EmbeddingsInputItem::ImageUrl {
                image_url: unigateway_core::ImageUrlObject {
                    url: "data:image/png;base64,AAAA".to_string(),
                },
            },
        ]);
        request.dimensions = Some(1024);
        let native = build_dashscope_multimodal_embeddings_request(&request).unwrap();
        assert_eq!(native["model"], "m");
        assert_eq!(native["input"]["contents"][0]["text"], "一张猫的照片");
        assert_eq!(
            native["input"]["contents"][1]["image"],
            "data:image/png;base64,AAAA"
        );
        assert_eq!(native["parameters"]["dimension"], 1024);
    }

    #[test]
    fn builds_volcengine_typed_items_shape() {
        let request = request_with(vec![
            EmbeddingsInputItem::Text {
                text: "一只猫".to_string(),
            },
            EmbeddingsInputItem::ImageUrl {
                image_url: unigateway_core::ImageUrlObject {
                    url: "https://example.com/cat.png".to_string(),
                },
            },
        ]);
        let native = build_volcengine_multimodal_embeddings_request(&request).unwrap();
        assert_eq!(native["input"][0]["type"], "text");
        assert_eq!(native["input"][1]["type"], "image_url");
        assert_eq!(
            native["input"][1]["image_url"]["url"],
            "https://example.com/cat.png"
        );
    }

    #[test]
    fn rejects_unsupported_surface_parameters() {
        let mut request = request_with(vec![EmbeddingsInputItem::Text {
            text: "x".to_string(),
        }]);
        request.encoding_format = Some("base64".to_string());
        assert!(
            build_dashscope_multimodal_embeddings_request(&request)
                .unwrap_err()
                .to_string()
                .contains("base64")
        );
        assert!(
            build_volcengine_multimodal_embeddings_request(&request)
                .unwrap_err()
                .to_string()
                .contains("base64")
        );

        let mut request = request_with(vec![EmbeddingsInputItem::Text {
            text: "x".to_string(),
        }]);
        request.dimensions = Some(256);
        assert!(
            build_volcengine_multimodal_embeddings_request(&request)
                .unwrap_err()
                .to_string()
                .contains("dimensions")
        );
    }

    #[test]
    fn converts_dashscope_response_to_openai_shape() {
        let raw = json!({
            "output": {
                "embeddings": [
                    { "embedding": [0.1, 0.2], "index": 0, "type": "text" },
                    { "embedding": [0.3], "text_index": 5 }
                ]
            },
            "usage": { "total_tokens": 7 },
            "request_id": "req-1"
        })
        .to_string();
        let converted = dashscope_multimodal_embeddings_response_to_openai(&raw, "m").unwrap();
        assert_eq!(converted["object"], "list");
        assert_eq!(converted["model"], "m");
        assert_eq!(converted["data"][0]["index"], 0);
        assert_eq!(converted["data"][1]["index"], 5, "text_index fallback");
        assert_eq!(converted["usage"]["total_tokens"], 7);

        let err = dashscope_multimodal_embeddings_response_to_openai("{\"output\": {}}", "m")
            .unwrap_err();
        assert!(err.to_string().contains("output.embeddings"));
    }

    #[test]
    fn converts_volcengine_single_object_response_to_openai_shape() {
        let raw = json!({
            "created": 1,
            "data": { "embedding": [0.1, 0.2], "object": "embedding" },
            "id": "req-2",
            "model": "doubao-embedding-vision-251215",
            "object": "list",
            "usage": { "prompt_tokens": 21, "total_tokens": 21 }
        })
        .to_string();
        let converted = volcengine_multimodal_embeddings_response_to_openai(&raw, "m").unwrap();
        assert_eq!(converted["object"], "list");
        assert_eq!(converted["data"][0]["embedding"][0], 0.1);
        assert_eq!(converted["data"][0]["index"], 0);
        assert_eq!(converted["usage"]["total_tokens"], 21);

        let err = volcengine_multimodal_embeddings_response_to_openai("{}", "m").unwrap_err();
        assert!(err.to_string().contains("data object"));
    }
}
