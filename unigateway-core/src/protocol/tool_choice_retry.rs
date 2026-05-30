use std::future::Future;

use crate::conversion::{
    UpstreamToolChoiceProtocol, apply_reactive_tool_choice_override,
    is_tool_choice_upstream_rejection, reactive_tool_choice_fallback,
    sent_tool_choice_from_metadata,
};
use crate::drivers::DriverEndpointContext;
use crate::error::GatewayError;
use crate::request::{ProxyChatRequest, TOOL_CHOICE_REACTIVE_RETRY_KEY};
use crate::transport::{StreamingTransportResponse, TransportRequest, TransportResponse};

pub(super) async fn send_with_tool_choice_retry<F, Fut>(
    mut endpoint: DriverEndpointContext,
    request: &ProxyChatRequest,
    protocol: UpstreamToolChoiceProtocol,
    mut build: F,
    send: impl Fn(TransportRequest) -> Fut,
) -> Result<(TransportResponse, DriverEndpointContext), GatewayError>
where
    F: FnMut(
        &mut DriverEndpointContext,
        &ProxyChatRequest,
    ) -> Result<TransportRequest, GatewayError>,
    Fut: Future<Output = Result<TransportResponse, GatewayError>>,
{
    let transport_request = build(&mut endpoint, request)?;
    let response = send(transport_request).await?;
    if (200..300).contains(&response.status) {
        return Ok((response, endpoint));
    }

    let body = String::from_utf8(response.body).ok();
    if endpoint
        .metadata
        .contains_key(TOOL_CHOICE_REACTIVE_RETRY_KEY)
        || !is_tool_choice_upstream_rejection(response.status, body.as_deref())
    {
        return Err(GatewayError::UpstreamHttp {
            status: response.status,
            body,
            endpoint_id: endpoint.endpoint_id,
        });
    }

    let sent = sent_tool_choice_from_metadata(&endpoint.metadata);
    let Some(next) = reactive_tool_choice_fallback(sent.as_ref(), protocol) else {
        return Err(GatewayError::UpstreamHttp {
            status: response.status,
            body,
            endpoint_id: endpoint.endpoint_id,
        });
    };

    apply_reactive_tool_choice_override(&mut endpoint.metadata, &next);
    let transport_request = build(&mut endpoint, request)?;
    let retry_response = send(transport_request).await?;
    if !(200..300).contains(&retry_response.status) {
        return Err(GatewayError::UpstreamHttp {
            status: retry_response.status,
            body: String::from_utf8(retry_response.body).ok(),
            endpoint_id: endpoint.endpoint_id,
        });
    }

    Ok((retry_response, endpoint))
}

pub(super) async fn send_stream_with_tool_choice_retry<F, Fut>(
    mut endpoint: DriverEndpointContext,
    request: &ProxyChatRequest,
    protocol: UpstreamToolChoiceProtocol,
    mut build: F,
    send_stream: impl Fn(TransportRequest) -> Fut,
) -> Result<(StreamingTransportResponse, DriverEndpointContext), GatewayError>
where
    F: FnMut(
        &mut DriverEndpointContext,
        &ProxyChatRequest,
    ) -> Result<TransportRequest, GatewayError>,
    Fut: Future<Output = Result<StreamingTransportResponse, GatewayError>>,
{
    let transport_request = build(&mut endpoint, request)?;
    match send_stream(transport_request).await {
        Ok(response) => Ok((response, endpoint)),
        Err(GatewayError::UpstreamHttp {
            status,
            body,
            endpoint_id,
        }) if !endpoint
            .metadata
            .contains_key(TOOL_CHOICE_REACTIVE_RETRY_KEY)
            && is_tool_choice_upstream_rejection(status, body.as_deref()) =>
        {
            let sent = sent_tool_choice_from_metadata(&endpoint.metadata);
            match reactive_tool_choice_fallback(sent.as_ref(), protocol) {
                Some(next) => {
                    apply_reactive_tool_choice_override(&mut endpoint.metadata, &next);
                    let transport_request = build(&mut endpoint, request)?;
                    send_stream(transport_request)
                        .await
                        .map(|response| (response, endpoint))
                }
                None => Err(GatewayError::UpstreamHttp {
                    status,
                    body,
                    endpoint_id,
                }),
            }
        }
        Err(error) => Err(error),
    }
}
