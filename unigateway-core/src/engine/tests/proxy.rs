use std::sync::Arc;

use crate::InMemoryDriverRegistry;
use crate::pool::ExecutionTarget;
use crate::response::ProxySession;

use super::super::UniGatewayEngine;
use super::support::{TestDriver, chat_request, endpoint, engine_with_empty_registry, pool};
use crate::retry::LoadBalancingStrategy;

#[tokio::test]
async fn proxy_chat_delegates_to_registered_driver() {
    let registry = Arc::new(InMemoryDriverRegistry::new());
    registry.register(Arc::new(TestDriver));

    let engine = UniGatewayEngine::builder()
        .with_driver_registry(registry)
        .build()
        .unwrap();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a")],
        ))
        .await
        .expect("upsert pool");

    let session = engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
        .expect("proxy chat");

    match session {
        ProxySession::Completed(result) => {
            assert_eq!(result.report.selected_endpoint_id, "a");
            assert_eq!(result.report.pool_id.as_deref(), Some("alpha"));
            assert_eq!(result.response.output_text.as_deref(), Some("a"));
        }
        ProxySession::Streaming(_) => panic!("expected completed response"),
    }
}

#[tokio::test]
async fn proxy_chat_fails_when_driver_missing() {
    let engine = engine_with_empty_registry();
    engine
        .upsert_pool(pool(
            "alpha",
            LoadBalancingStrategy::RoundRobin,
            vec![endpoint("a")],
        ))
        .await
        .expect("upsert pool");

    let error = match engine
        .proxy_chat(
            chat_request(false),
            ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            },
        )
        .await
    {
        Ok(_) => panic!("missing driver registry should fail"),
        Err(error) => error,
    };

    assert!(
        error
            .to_string()
            .contains("driver not found: openai-compatible")
    );
}
