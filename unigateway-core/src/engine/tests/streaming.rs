use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;

use crate::InMemoryDriverRegistry;
use crate::pool::ExecutionTarget;
use crate::response::{ProxySession, StreamOutcome};
use crate::retry::LoadBalancingStrategy;

use super::super::UniGatewayEngine;
use super::support::{HookRecorder, StreamingDriver, chat_request, endpoint, pool};

#[tokio::test]
async fn hooks_receive_stream_lifecycle_events_for_streaming_chat() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(StreamingDriver));

    let hooks = HookRecorder::default();
    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .with_hooks(Arc::new(hooks.clone()))
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("streamer")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_chat(
            chat_request(true),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .expect("proxy chat stream");

    let ProxySession::Streaming(mut streaming) = session else {
        panic!("expected streaming response");
    };

    let mut deltas = Vec::new();
    while let Some(chunk) = streaming.stream.next().await {
        deltas.push(chunk.expect("chunk ok").delta.expect("delta"));
    }
    let completed = streaming
        .completion
        .await
        .expect("completion channel")
        .expect("completed stream");

    assert_eq!(deltas, vec!["hel".to_string(), "lo".to_string()]);
    assert_eq!(completed.response.output_text.as_deref(), Some("hello"));
    assert_eq!(
        completed
            .report
            .stream
            .as_ref()
            .map(|report| report.chunk_count),
        Some(2)
    );
    assert_eq!(
        completed
            .report
            .stream
            .as_ref()
            .map(|report| report.outcome),
        Some(StreamOutcome::Completed)
    );

    assert_eq!(hooks.state.request_started.lock().unwrap().len(), 1);
    assert_eq!(hooks.state.started.lock().unwrap().len(), 1);
    assert_eq!(hooks.state.stream_started.lock().unwrap().len(), 1);
    assert_eq!(hooks.state.stream_chunks.lock().unwrap().len(), 2);
    assert_eq!(hooks.state.stream_completed.lock().unwrap().len(), 1);
    assert!(hooks.state.stream_aborted.lock().unwrap().is_empty());
    assert_eq!(hooks.state.requests.lock().unwrap().len(), 1);

    let stream_chunks = hooks.state.stream_chunks.lock().unwrap();
    assert!(stream_chunks[0].first_chunk);
    assert!(!stream_chunks[1].first_chunk);
    assert!(stream_chunks[0].ttft_ms.is_some());
}

#[tokio::test]
async fn streaming_completion_resolves_without_draining_output_stream() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(StreamingDriver));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("streamer")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_chat(
            chat_request(true),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .expect("proxy chat stream");

    let ProxySession::Streaming(streaming) = session else {
        panic!("expected streaming response");
    };
    let completed = tokio::time::timeout(Duration::from_millis(200), streaming.into_completion())
        .await
        .expect("completion should not depend on stream draining")
        .expect("completed stream");

    assert_eq!(completed.response.output_text.as_deref(), Some("hello"));
    assert_eq!(
        completed
            .report
            .stream
            .as_ref()
            .map(|report| report.chunk_count),
        Some(2)
    );
}
