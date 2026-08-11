use crate::store::{PublishResult, SessionKey};

/// Neutral session lifecycle events for observability hooks.
///
/// Events must not include prompt text, tool output, or credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionLifecycleEvent {
    PublishCreated {
        key: SessionKey,
        epoch: u64,
        message_count: u64,
        bytes: usize,
    },
    PublishReplaced {
        key: SessionKey,
        epoch: u64,
        message_count: u64,
        bytes: usize,
    },
    PublishIdempotent {
        key: SessionKey,
        epoch: u64,
    },
    StalePublish {
        key: SessionKey,
        existing_epoch: u64,
        attempted_epoch: u64,
    },
    EpochConflict {
        key: SessionKey,
        epoch: u64,
    },
    DeltaHit {
        key: SessionKey,
        epoch: u64,
    },
    DeltaMiss {
        key: SessionKey,
    },
    SessionExpired {
        key: SessionKey,
    },
    SessionDeleted {
        key: SessionKey,
    },
    FingerprintMismatch {
        key: SessionKey,
    },
    TailMismatch {
        key: SessionKey,
    },
    SizeRejected {
        key: SessionKey,
        kind: SessionSizeRejectKind,
    },
    StoreUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSizeRejectKind {
    Prefix,
    Tail,
    Assembled,
}

impl SessionLifecycleEvent {
    pub fn from_publish_result(
        key: SessionKey,
        result: PublishResult,
        epoch: u64,
        message_count: u64,
        bytes: usize,
    ) -> Self {
        match result {
            PublishResult::Created => Self::PublishCreated {
                key,
                epoch,
                message_count,
                bytes,
            },
            PublishResult::Replaced => Self::PublishReplaced {
                key,
                epoch,
                message_count,
                bytes,
            },
            PublishResult::AlreadyCurrent => Self::PublishIdempotent { key, epoch },
        }
    }
}

/// Optional hook for session lifecycle observability.
pub trait SessionLifecycleHook: Send + Sync {
    fn on_event(&self, event: SessionLifecycleEvent);
}
