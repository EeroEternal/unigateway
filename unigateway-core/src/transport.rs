use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

use futures_core::Stream;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use serde::Serialize;

use crate::error::GatewayError;
use crate::pool::EndpointId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportRequest {
    pub endpoint_id: Option<EndpointId>,
    pub method: HttpMethod,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
}

impl TransportRequest {
    pub fn post_json<T: Serialize>(
        endpoint_id: Option<EndpointId>,
        url: String,
        headers: HashMap<String, String>,
        body: &T,
        timeout: Option<Duration>,
    ) -> Result<Self, GatewayError> {
        let body = serde_json::to_vec(body).map_err(|error| {
            GatewayError::InvalidRequest(format!("failed to serialize request body: {error}"))
        })?;

        Ok(Self {
            endpoint_id,
            method: HttpMethod::Post,
            url,
            headers,
            body: Some(body),
            timeout,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub type TransportByteStream =
    Pin<Box<dyn Stream<Item = Result<Vec<u8>, GatewayError>> + Send + 'static>>;

pub struct StreamingTransportResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub stream: TransportByteStream,
}

pub trait HttpTransport: Send + Sync + 'static {
    fn send(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, GatewayError>>;

    fn send_stream(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<StreamingTransportResponse, GatewayError>>;
}

pub struct ReqwestHttpTransport {
    client: reqwest::Client,
}

impl ReqwestHttpTransport {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestHttpTransport {
    fn default() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl HttpTransport for ReqwestHttpTransport {
    fn send(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<TransportResponse, GatewayError>> {
        let client = self.client.clone();

        Box::pin(async move {
            let mut builder = match request.method {
                HttpMethod::Get => client.get(&request.url),
                HttpMethod::Post => client.post(&request.url),
            };

            if let Some(timeout) = request.timeout {
                builder = builder.timeout(timeout);
            }

            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }

            if let Some(body) = request.body {
                builder = builder.body(body);
            }

            let endpoint_id = request.endpoint_id.clone();
            let response = builder
                .send()
                .await
                .map_err(|error| GatewayError::Transport {
                    message: error.to_string(),
                    endpoint_id: endpoint_id.clone(),
                })?;

            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_string(), value.to_string()))
                })
                .collect();
            let body = response
                .bytes()
                .await
                .map_err(|error| GatewayError::Transport {
                    message: error.to_string(),
                    endpoint_id,
                })?;

            Ok(TransportResponse {
                status,
                headers,
                body: body.to_vec(),
            })
        })
    }

    fn send_stream(
        &self,
        request: TransportRequest,
    ) -> BoxFuture<'static, Result<StreamingTransportResponse, GatewayError>> {
        let client = self.client.clone();

        Box::pin(async move {
            let mut builder = match request.method {
                HttpMethod::Get => client.get(&request.url),
                HttpMethod::Post => client.post(&request.url),
            };

            if let Some(timeout) = request.timeout {
                builder = builder.timeout(timeout);
            }

            for (name, value) in &request.headers {
                builder = builder.header(name, value);
            }

            if let Some(body) = request.body {
                builder = builder.body(body);
            }

            let endpoint_id = request.endpoint_id.clone();
            let response = builder
                .send()
                .await
                .map_err(|error| GatewayError::Transport {
                    message: error.to_string(),
                    endpoint_id: endpoint_id.clone(),
                })?;

            let status = response.status().as_u16();
            let headers: HashMap<String, String> = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|value| (name.as_str().to_string(), value.to_string()))
                })
                .collect();

            if !(200..300).contains(&status) {
                let body = response
                    .bytes()
                    .await
                    .map_err(|error| GatewayError::Transport {
                        message: error.to_string(),
                        endpoint_id: endpoint_id.clone(),
                    })?;

                return Err(GatewayError::UpstreamHttp {
                    status,
                    body: String::from_utf8(body.to_vec()).ok(),
                    endpoint_id: endpoint_id.unwrap_or_else(|| "unknown".to_string()),
                });
            }

            let stream_endpoint_id = endpoint_id.clone();
            let stream = response.bytes_stream().map(move |item| {
                item.map(|bytes| bytes.to_vec())
                    .map_err(|error| GatewayError::Transport {
                        message: error.to_string(),
                        endpoint_id: stream_endpoint_id.clone(),
                    })
            });

            Ok(StreamingTransportResponse {
                status,
                headers,
                stream: Box::pin(stream),
            })
        })
    }
}
