//! gRPC client skeleton for sglang-lite.
//!
//! Enabled via the `sglang-lite-grpc` feature (which implies `sglang-lite`).
//! The protobuf contract is vendored from sglang-lite and compiled at build time.
//!
//! This is a skeleton: basic chat (non-stream + stream) is mapped.
//! Advanced fields (tools, reasoning, etc.) are not yet supported in the proto
//! and will be ignored or cause fallback in future iterations.
//!
//! See docs/guide/sglang-lite.md and the original spec in sglang-lite repo.

use std::pin::Pin;
use std::time::SystemTime;

use futures_core::Stream;
use futures_util::StreamExt;
use serde_json::json;
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant, sleep, timeout};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::transport::Channel;

use super::backend::SglangLiteSubprocessConfig;
use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::{MessageRole, ProxyChatRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, ProxySession, StreamingResponse,
    TokenUsage,
};

pub mod proto {
    tonic::include_proto!("sglang_lite");
}

use proto::sglang_lite_service_client::SglangLiteServiceClient;
use proto::{ChatCompletionsRequest, Message as GrpcMessage, Usage};

/// Thin wrapper around the generated gRPC client.
#[derive(Clone)]
pub struct SglangLiteGrpcClient {
    client: SglangLiteServiceClient<Channel>,
}

impl SglangLiteGrpcClient {
    /// Connect to the gRPC endpoint (e.g. "http://127.0.0.1:50051").
    pub async fn connect(base_url: &str) -> Result<Self, GatewayError> {
        let endpoint =
            tonic::transport::Endpoint::from_shared(base_url.to_string()).map_err(|e| {
                GatewayError::Transport {
                    message: format!("invalid gRPC endpoint {base_url}: {e}"),
                    endpoint_id: None,
                }
            })?;

        let channel = endpoint
            .connect()
            .await
            .map_err(|e| GatewayError::Transport {
                message: format!("failed to connect to sglang-lite gRPC at {base_url}: {e}"),
                endpoint_id: None,
            })?;

        let client = SglangLiteServiceClient::new(channel);
        Ok(Self { client })
    }

    /// Perform a non-streaming chat completion.
    pub async fn chat(
        &self,
        req: &ProxyChatRequest,
    ) -> Result<(ChatResponseFinal, Option<TokenUsage>), GatewayError> {
        let grpc_req = build_grpc_request(req);
        let response = self
            .client
            .clone()
            .chat_completions(grpc_req)
            .await
            .map_err(|status| map_grpc_error(status, "ChatCompletions"))?;

        let inner = response.into_inner();
        Ok(convert_response(inner))
    }

    /// Perform a streaming chat completion.
    /// Returns a stream of ChatResponseChunk. Final usage (if sent by server in last chunk)
    /// is included inside the raw JSON of the chunk for now (skeleton limitation).
    #[allow(dead_code)]
    pub async fn chat_stream(
        &self,
        req: &ProxyChatRequest,
    ) -> Result<
        Pin<Box<dyn Stream<Item = Result<ChatResponseChunk, GatewayError>> + Send + 'static>>,
        GatewayError,
    > {
        let grpc_req = build_grpc_request(req);
        let response_stream = self
            .client
            .clone()
            .chat_completions_stream(grpc_req)
            .await
            .map_err(|status| map_grpc_error(status, "ChatCompletionsStream"))?
            .into_inner();

        let mapped = response_stream.map(|chunk_res| match chunk_res {
            Ok(chunk) => Ok(convert_chunk(chunk)),
            Err(status) => Err(map_grpc_error(status, "ChatCompletionsStream chunk")),
        });

        Ok(Box::pin(mapped))
    }
}

fn build_grpc_request(req: &ProxyChatRequest) -> ChatCompletionsRequest {
    let messages = req
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool",
            }
            .to_string();

            // For skeleton we only support simple text content.
            // Real tool calls / multi-part would require proto evolution.
            let content = m
                .content
                .iter()
                .filter_map(|block| match block {
                    crate::request::ContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            // Basic tool_call_id for tool role messages (if content carries the id)
            // Map tool_call_id from ToolResult blocks for "tool" role messages.
            // tool_call_id should be the id of the corresponding tool call, not the result content.
            let tool_call_id = if role == "tool" {
                m.content.iter().find_map(|block| {
                    if let crate::request::ContentBlock::ToolResult { tool_use_id, .. } = block {
                        Some(tool_use_id.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            // Map tool_calls from ToolUse blocks for assistant messages (skeleton support).
            let tool_calls: Vec<proto::ToolCall> = m
                .content
                .iter()
                .filter_map(|block| {
                    if let crate::request::ContentBlock::ToolUse { id, name, input } = block {
                        Some(proto::ToolCall {
                            id: id.clone(),
                            r#type: "function".to_string(),
                            function: Some(proto::FunctionCall {
                                name: name.clone(),
                                arguments: input.to_string(),
                            }),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            GrpcMessage {
                role,
                content,
                tool_calls,
                tool_call_id,
            }
        })
        .collect();

    let stop = match &req.stop_sequences {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        Some(serde_json::Value::String(s)) => vec![s.clone()],
        _ => vec![],
    };

    // Basic tools mapping from Value (skeleton)
    let tools = match &req.tools {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|t| {
                let func = t.get("function")?;
                Some(proto::Tool {
                    r#type: t
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("function")
                        .to_string(),
                    function: Some(proto::FunctionDefinition {
                        name: func
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: func
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        parameters: func
                            .get("parameters")
                            .map(|v| v.to_string())
                            .unwrap_or_default(),
                    }),
                })
            })
            .collect(),
        _ => vec![],
    };

    let tool_choice = if let Some(v) = req.extra.get("tool_choice").or(req.tool_choice.as_ref()) {
        match v {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(_) => Some(v.to_string()), // serialized object
            _ => Some(v.to_string()),
        }
    } else {
        req.metadata.get("tool_choice").cloned()
    };

    ChatCompletionsRequest {
        model: req.model.clone(),
        messages,
        temperature: req.temperature,
        top_p: req.top_p,
        top_k: req.top_k.map(|v| v as i32),
        max_tokens: req.max_tokens.map(|v| v as i32),
        stop,
        stream: req.stream,
        tools,
        tool_choice,
    }
}

fn convert_response(
    resp: proto::ChatCompletionsResponse,
) -> (ChatResponseFinal, Option<TokenUsage>) {
    let output_text = resp
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .map(|m| m.content.clone());

    let raw = build_openai_like_raw(&resp);

    let usage = resp.usage.as_ref().map(convert_usage);

    (
        ChatResponseFinal {
            model: Some(resp.model),
            output_text,
            raw,
        },
        usage,
    )
}

#[allow(dead_code)]
fn convert_chunk(chunk: proto::ChatCompletionChunk) -> ChatResponseChunk {
    let delta = chunk
        .choices
        .first()
        .and_then(|c| c.delta.as_ref())
        .and_then(|d| d.content.clone());

    let raw = json!({
        "id": chunk.id,
        "object": chunk.object,
        "created": chunk.created,
        "model": chunk.model,
        "choices": chunk.choices.iter().map(|c| {
            json!({
                "index": c.index,
                "delta": {
                "role": c.delta.as_ref().and_then(|d| d.role.clone()),
                "content": c.delta.as_ref().and_then(|d| d.content.clone()),
                },
                "finish_reason": c.finish_reason,
            })
        }).collect::<Vec<_>>(),
    });

    ChatResponseChunk { delta, raw }
}

fn convert_usage(u: &Usage) -> TokenUsage {
    TokenUsage {
        input_tokens: Some(u.prompt_tokens as u64),
        output_tokens: Some(u.completion_tokens as u64),
        total_tokens: Some(u.total_tokens as u64),
        reasoning_tokens: None,
        cache_hit_tokens: u.cache_hit_tokens.map(|v| v as u64),
        cache_write_tokens: None,
    }
}

fn build_openai_like_raw(resp: &proto::ChatCompletionsResponse) -> serde_json::Value {
    // Build a shape compatible with the existing OpenAI passthrough / synthetic renderers.
    let choices = resp
        .choices
        .iter()
        .map(|c| {
            json!({
                "index": c.index,
                "message": {
                    "role": c.message.as_ref().map(|m| m.role.clone()).unwrap_or_default(),
                    "content": c.message.as_ref().map(|m| m.content.clone()).unwrap_or_default(),
                },
                "finish_reason": c.finish_reason,
            })
        })
        .collect::<Vec<_>>();

    let mut usage = json!({
        "prompt_tokens": resp.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
        "completion_tokens": resp.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
        "total_tokens": resp.usage.as_ref().map(|u| u.total_tokens).unwrap_or(0),
    });

    if let Some(u) = &resp.usage
        && let Some(ch) = u.cache_hit_tokens
        && let Some(obj) = usage.as_object_mut()
    {
        obj.insert("cache_hit_tokens".to_string(), json!(ch));
    }

    json!({
        "id": resp.id,
        "object": resp.object,
        "created": resp.created,
        "model": resp.model,
        "choices": choices,
        "usage": usage,
    })
}

fn map_grpc_error(status: tonic::Status, context: &str) -> GatewayError {
    let message = format!(
        "sglang-lite gRPC {context} failed: {} ({})",
        status.message(),
        status.code()
    );
    match status.code() {
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded => GatewayError::Transport {
            message,
            endpoint_id: None,
        },
        tonic::Code::InvalidArgument | tonic::Code::FailedPrecondition => {
            GatewayError::InvalidRequest(message)
        }
        tonic::Code::Unimplemented => {
            GatewayError::not_implemented("sglang-lite grpc feature unimplemented")
        }
        _ => GatewayError::Transport {
            message,
            endpoint_id: None,
        },
    }
}

/// Drive the gRPC chunk stream, forward chunks, accumulate state, and send final completion.
async fn drive_grpc_chat_stream(
    mut grpc_stream: Pin<Box<dyn Stream<Item = Result<ChatResponseChunk, GatewayError>> + Send>>,
    chunk_tx: mpsc::UnboundedSender<Result<ChatResponseChunk, GatewayError>>,
    endpoint: DriverEndpointContext,
    started_at: SystemTime,
    request_id: String,
) -> Result<CompletedResponse<ChatResponseFinal>, GatewayError> {
    use crate::protocol::build_request_report;
    use crate::response::RequestKind;

    let mut output_text = String::new();
    let mut last_raw = json!({});
    let mut usage: Option<TokenUsage> = None;
    let mut model: Option<String> = None;

    while let Some(chunk_res) = grpc_stream.next().await {
        match chunk_res {
            Ok(chunk) => {
                if let Some(delta) = &chunk.delta {
                    output_text.push_str(delta);
                }
                // Extract usage / model from chunk raw if present (last chunk often has usage)
                if let Some(u_val) = chunk.raw.get("usage")
                    && let Some(u) = parse_usage_from_value(u_val)
                {
                    usage = Some(u);
                }
                if model.is_none()
                    && let Some(m) = chunk.raw.get("model").and_then(|v| v.as_str())
                {
                    model = Some(m.to_string());
                }
                last_raw = chunk.raw.clone();
                if chunk_tx.send(Ok(chunk)).is_err() {
                    break; // downstream dropped
                }
            }
            Err(e) => {
                let _ = chunk_tx.send(Err(e));
                break;
            }
        }
    }

    let finished_at = SystemTime::now();

    let final_resp = ChatResponseFinal {
        model,
        output_text: if output_text.is_empty() {
            None
        } else {
            Some(output_text)
        },
        raw: last_raw,
    };

    let report = build_request_report(
        &endpoint,
        started_at,
        finished_at,
        usage,
        RequestKind::Chat,
        Some(request_id),
    );

    Ok(CompletedResponse {
        response: final_resp,
        report,
    })
}

fn parse_usage_from_value(v: &serde_json::Value) -> Option<TokenUsage> {
    Some(TokenUsage {
        input_tokens: v.get("prompt_tokens").and_then(|x| x.as_u64()),
        output_tokens: v.get("completion_tokens").and_then(|x| x.as_u64()),
        total_tokens: v.get("total_tokens").and_then(|x| x.as_u64()),
        reasoning_tokens: v
            .get("output_tokens_details")
            .and_then(|d| d.get("reasoning_tokens"))
            .and_then(|x| x.as_u64()),
        cache_hit_tokens: v.get("cache_hit_tokens").and_then(|x| x.as_u64()),
        cache_write_tokens: v.get("cache_write_tokens").and_then(|x| x.as_u64),
    })
}

/// Spawn process for gRPC backend and wait using standard gRPC health v1 service.
pub async fn spawn_and_wait_grpc_health(
    config: SglangLiteSubprocessConfig,
) -> Result<tokio::process::Child, GatewayError> {
    let mut child = Command::new(&config.command)
        .args(&config.args)
        .spawn()
        .map_err(|e| GatewayError::Transport {
            message: format!("failed to spawn sglang-lite grpc subprocess: {e}"),
            endpoint_id: None,
        })?;

    let deadline = Instant::now() + Duration::from_millis(config.startup_timeout_ms);
    let base = config.base_url.clone();

    let wait_res = timeout(Duration::from_millis(config.startup_timeout_ms), async {
        loop {
            if check_grpc_health(&base).await.is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(());
            }
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await;

    match wait_res {
        Ok(Ok(())) => Ok(child),
        _ => {
            let _ = child.kill().await;
            Err(GatewayError::Transport {
                message: format!(
                    "sglang-lite grpc subprocess did not become ready within {} ms",
                    config.startup_timeout_ms
                ),
                endpoint_id: None,
            })
        }
    }
}

async fn check_grpc_health(base_url: &str) -> Result<(), GatewayError> {
    let channel = tonic::transport::Endpoint::from_shared(base_url.to_string())
        .map_err(|e| GatewayError::Transport {
            message: format!("invalid grpc url: {e}"),
            endpoint_id: None,
        })?
        .connect()
        .await
        .map_err(|e| GatewayError::Transport {
            message: format!("grpc connect failed: {e}"),
            endpoint_id: None,
        })?;

    // Use standard gRPC health v1 (matches sglang-lite spec and unigateway guide)
    let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
    let resp = client
        .check(tonic_health::pb::HealthCheckRequest {
            service: String::new(),
        })
        .await
        .map_err(|e| GatewayError::Transport {
            message: format!("grpc health check call failed: {e}"),
            endpoint_id: None,
        })?;

    if resp.get_ref().status
        == tonic_health::pb::health_check_response::ServingStatus::Serving as i32
    {
        Ok(())
    } else {
        Err(GatewayError::Transport {
            message: "grpc backend not serving".to_string(),
            endpoint_id: None,
        })
    }
}

/// Entry point called from SglangLiteDriver when backend == Grpc.
/// Supports both unary and streaming with proper completion handle.
pub async fn execute_chat_grpc(
    endpoint: crate::drivers::DriverEndpointContext,
    request: ProxyChatRequest,
) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
    let client = SglangLiteGrpcClient::connect(&endpoint.base_url).await?;

    if request.stream {
        let request_id = crate::protocol::next_request_id();
        let started_at = SystemTime::now();
        let mut request_metadata = request.metadata.clone();
        request_metadata.extend(endpoint.metadata.clone());

        let grpc_stream = client.chat_stream(&request).await?;

        let (chunk_tx, chunk_rx) =
            mpsc::unbounded_channel::<Result<ChatResponseChunk, GatewayError>>();
        let (completion_tx, completion_rx) = oneshot::channel();
        let completion_request_id = request_id.clone();
        let endpoint_for_drive = endpoint.clone();

        tokio::spawn(async move {
            let completion = drive_grpc_chat_stream(
                grpc_stream,
                chunk_tx,
                endpoint_for_drive,
                started_at,
                completion_request_id,
            )
            .await;
            let _ = completion_tx.send(completion);
        });

        Ok(ProxySession::Streaming(StreamingResponse {
            stream: Box::pin(UnboundedReceiverStream::new(chunk_rx)),
            completion: completion_rx,
            request_id,
            request_metadata,
        }))
    } else {
        // unary
        let started_at = SystemTime::now();
        let (final_resp, usage) = client.chat(&request).await?;
        let finished_at = SystemTime::now();

        let report = crate::protocol::build_request_report(
            &endpoint,
            started_at,
            finished_at,
            usage,
            crate::response::RequestKind::Chat,
            None,
        );

        Ok(ProxySession::Completed(CompletedResponse {
            response: final_resp,
            report,
        }))
    }
}
