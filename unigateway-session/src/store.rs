use std::collections::HashMap;
use std::sync::RwLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::lifecycle::{SessionLifecycleEvent, SessionLifecycleHook};
use crate::lifetime::{SessionLifetime, session_expired};

/// Default namespace for bare `session_id` compatibility APIs.
pub const DEFAULT_NAMESPACE: &str = "default";

/// Host-provided isolation boundary plus opaque client session id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub namespace: String,
    pub session_id: String,
}

impl SessionKey {
    pub fn new(namespace: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            session_id: session_id.into(),
        }
    }

    pub fn default_namespace(session_id: impl Into<String>) -> Self {
        Self::new(DEFAULT_NAMESPACE, session_id)
    }

    pub fn storage_key(&self) -> String {
        format!("{}\0{}", self.namespace, self.session_id)
    }
}

/// Opaque prefix fingerprint; algorithm and value are host-defined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub algorithm: String,
    pub value: String,
}

/// Whether delta/publish paths require fingerprint validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FingerprintPolicy {
    #[default]
    Disabled,
    Optional,
    Required,
}

/// Optional byte/message limits for prefix, tail, and assembled payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionSizeLimits {
    pub max_messages: Option<usize>,
    pub max_prefix_bytes: Option<usize>,
    pub max_tail_bytes: Option<usize>,
    pub max_assembled_bytes: Option<usize>,
}

impl SessionSizeLimits {
    pub fn validate_prefix(
        &self,
        key: &SessionKey,
        messages: &[Value],
    ) -> Result<(), SessionError> {
        if let Some(max) = self.max_messages
            && messages.len() > max
        {
            return Err(SessionError::PrefixTooLarge {
                key: key.clone(),
                limit: max,
                actual: messages.len(),
            });
        }

        if let Some(max) = self.max_prefix_bytes {
            let actual = message_json_bytes(messages)?;
            if actual > max {
                return Err(SessionError::PrefixTooLarge {
                    key: key.clone(),
                    limit: max,
                    actual,
                });
            }
        }

        Ok(())
    }

    pub fn validate_tail(&self, key: &SessionKey, tail: &[Value]) -> Result<(), SessionError> {
        let Some(max) = self.max_tail_bytes else {
            return Ok(());
        };
        let actual = message_json_bytes(tail)?;
        if actual > max {
            return Err(SessionError::TailTooLarge {
                key: key.clone(),
                limit: max,
                actual,
            });
        }
        Ok(())
    }

    pub fn validate_assembled(
        &self,
        key: &SessionKey,
        assembled: &[Value],
    ) -> Result<(), SessionError> {
        let Some(max) = self.max_assembled_bytes else {
            return Ok(());
        };
        let actual = message_json_bytes(assembled)?;
        if actual > max {
            return Err(SessionError::AssembledTooLarge {
                key: key.clone(),
                limit: max,
                actual,
            });
        }
        Ok(())
    }
}

/// Configuration for reference in-memory store implementations.
#[derive(Clone, Default)]
pub struct SessionStoreConfig {
    pub size_limits: SessionSizeLimits,
    pub lifetime: SessionLifetime,
    pub lifecycle_hook: Option<std::sync::Arc<dyn SessionLifecycleHook>>,
}

impl SessionStoreConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Stored conversation prefix for delta assembly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPrefix {
    pub epoch: u64,
    pub messages: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_boundary: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<Fingerprint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_count: Option<u64>,
}

impl SessionPrefix {
    /// Fills default `message_count` from `messages.len()` when omitted.
    pub fn normalize(mut self) -> Self {
        if self.message_count.is_none() {
            self.message_count = Some(self.messages.len() as u64);
        }
        self
    }

    fn normalized(self) -> Self {
        self.normalize()
    }
}

#[derive(Debug, Clone)]
struct StoredSession {
    prefix: SessionPrefix,
    created_at: SystemTime,
    last_accessed_at: SystemTime,
}

impl StoredSession {
    fn new(prefix: SessionPrefix, now: SystemTime) -> Self {
        Self {
            prefix,
            created_at: now,
            last_accessed_at: now,
        }
    }

    fn touch(&mut self, now: SystemTime) {
        self.last_accessed_at = now;
    }

    fn replace_prefix(&mut self, prefix: SessionPrefix, now: SystemTime) {
        self.prefix = prefix;
        self.created_at = now;
        self.last_accessed_at = now;
    }
}

/// Outcome of a successful publish operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishResult {
    Created,
    Replaced,
    AlreadyCurrent,
}

/// Stable session store / consistency errors (not HTTP-specific).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    NotFound(SessionKey),
    Expired(SessionKey),
    StaleEpoch {
        key: SessionKey,
        existing_epoch: u64,
        attempted_epoch: u64,
    },
    EpochConflict {
        key: SessionKey,
        epoch: u64,
    },
    EpochMismatch {
        key: SessionKey,
        expected_epoch: u64,
        actual_epoch: u64,
    },
    FingerprintMismatch {
        key: SessionKey,
    },
    TailStartMismatch {
        key: SessionKey,
        expected: u64,
        actual: u64,
    },
    PrefixTooLarge {
        key: SessionKey,
        limit: usize,
        actual: usize,
    },
    TailTooLarge {
        key: SessionKey,
        limit: usize,
        actual: usize,
    },
    AssembledTooLarge {
        key: SessionKey,
        limit: usize,
        actual: usize,
    },
    InvalidContext(String),
    Unavailable(String),
}

/// Backward-compatible alias for early reference releases.
pub type SessionStoreError = SessionError;

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(key) => write!(f, "session not found: {}", key.session_id),
            Self::Expired(key) => write!(f, "session expired: {}", key.session_id),
            Self::StaleEpoch {
                key,
                existing_epoch,
                attempted_epoch,
            } => write!(
                f,
                "stale epoch for session {}: existing {existing_epoch}, attempted {attempted_epoch}",
                key.session_id
            ),
            Self::EpochConflict { key, epoch } => {
                write!(
                    f,
                    "epoch conflict for session {} at epoch {epoch}",
                    key.session_id
                )
            }
            Self::EpochMismatch {
                key,
                expected_epoch,
                actual_epoch,
            } => write!(
                f,
                "epoch mismatch for session {}: expected {expected_epoch}, got {actual_epoch}",
                key.session_id
            ),
            Self::FingerprintMismatch { key } => {
                write!(f, "fingerprint mismatch for session {}", key.session_id)
            }
            Self::TailStartMismatch {
                key,
                expected,
                actual,
            } => write!(
                f,
                "tail_start mismatch for session {}: expected {expected}, got {actual}",
                key.session_id
            ),
            Self::PrefixTooLarge { key, limit, actual } => write!(
                f,
                "prefix too large for session {}: limit {limit}, actual {actual}",
                key.session_id
            ),
            Self::TailTooLarge { key, limit, actual } => write!(
                f,
                "tail too large for session {}: limit {limit}, actual {actual}",
                key.session_id
            ),
            Self::AssembledTooLarge { key, limit, actual } => write!(
                f,
                "assembled request too large for session {}: limit {limit}, actual {actual}",
                key.session_id
            ),
            Self::InvalidContext(message) => write!(f, "invalid session context: {message}"),
            Self::Unavailable(message) => write!(f, "session store unavailable: {message}"),
        }
    }
}

impl std::error::Error for SessionError {}

pub fn message_json_bytes(messages: &[Value]) -> Result<usize, SessionError> {
    serde_json::to_vec(messages)
        .map(|bytes| bytes.len())
        .map_err(|error| SessionError::InvalidContext(error.to_string()))
}

pub fn fingerprints_match(stored: &Fingerprint, request: &Fingerprint) -> bool {
    if !stored.algorithm.is_empty()
        && !request.algorithm.is_empty()
        && stored.algorithm != request.algorithm
    {
        return false;
    }
    stored.value == request.value
}

/// Pluggable session prefix store.
pub trait SessionStore: Send + Sync {
    fn publish_key(
        &self,
        key: &SessionKey,
        prefix: SessionPrefix,
    ) -> Result<PublishResult, SessionError>;

    fn get_key(&self, key: &SessionKey) -> Result<Option<SessionPrefix>, SessionError>;

    fn delete_key(&self, key: &SessionKey) -> Result<(), SessionError>;

    fn touch_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        let _ = key;
        Ok(())
    }

    fn purge_expired(&self) -> Result<usize, SessionError> {
        Ok(0)
    }
}

/// In-memory session prefix store (reference implementation).
pub struct MemorySessionStore {
    inner: RwLock<HashMap<String, StoredSession>>,
    config: SessionStoreConfig,
}

impl Default for MemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::with_config(SessionStoreConfig::default())
    }

    pub fn with_config(config: SessionStoreConfig) -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
            config,
        }
    }

    fn emit(&self, event: SessionLifecycleEvent) {
        if let Some(hook) = &self.config.lifecycle_hook {
            hook.on_event(event);
        }
    }

    fn lock(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, HashMap<String, StoredSession>>, SessionError> {
        self.inner
            .write()
            .map_err(|_| SessionError::Unavailable("lock poisoned".to_string()))
    }

    fn remove_if_expired(
        &self,
        guard: &mut std::sync::RwLockWriteGuard<'_, HashMap<String, StoredSession>>,
        key: &SessionKey,
        storage_key: &str,
        now: SystemTime,
    ) -> Result<(), SessionError> {
        let Some(entry) = guard.get(storage_key) else {
            return Ok(());
        };
        if session_expired(
            entry.created_at,
            entry.last_accessed_at,
            &self.config.lifetime,
            now,
        ) {
            guard.remove(storage_key);
            self.emit(SessionLifecycleEvent::SessionExpired { key: key.clone() });
            return Err(SessionError::Expired(key.clone()));
        }
        Ok(())
    }

    pub fn publish_key(
        &self,
        key: &SessionKey,
        prefix: SessionPrefix,
    ) -> Result<PublishResult, SessionError> {
        let prefix = prefix.normalized();
        self.config
            .size_limits
            .validate_prefix(key, &prefix.messages)?;

        let message_count = prefix.message_count.unwrap_or(prefix.messages.len() as u64);
        let bytes = message_json_bytes(&prefix.messages)?;
        let epoch = prefix.epoch;
        let now = SystemTime::now();
        let mut guard = self.lock()?;
        let storage_key = key.storage_key();

        if let Err(error) = self.remove_if_expired(&mut guard, key, &storage_key, now)
            && !matches!(error, SessionError::Expired(_))
        {
            return Err(error);
        }
        // Expired entry removed; continue as create when `Expired`.

        let result = match guard.get(&storage_key) {
            None => {
                guard.insert(storage_key, StoredSession::new(prefix, now));
                PublishResult::Created
            }
            Some(existing) if prefix.epoch > existing.prefix.epoch => {
                guard
                    .get_mut(&storage_key)
                    .expect("entry exists")
                    .replace_prefix(prefix, now);
                PublishResult::Replaced
            }
            Some(existing) if prefix.epoch < existing.prefix.epoch => {
                self.emit(SessionLifecycleEvent::StalePublish {
                    key: key.clone(),
                    existing_epoch: existing.prefix.epoch,
                    attempted_epoch: prefix.epoch,
                });
                return Err(SessionError::StaleEpoch {
                    key: key.clone(),
                    existing_epoch: existing.prefix.epoch,
                    attempted_epoch: prefix.epoch,
                });
            }
            Some(existing) if existing.prefix == prefix => {
                guard
                    .get_mut(&storage_key)
                    .expect("entry exists")
                    .touch(now);
                PublishResult::AlreadyCurrent
            }
            Some(_) => {
                self.emit(SessionLifecycleEvent::EpochConflict {
                    key: key.clone(),
                    epoch: prefix.epoch,
                });
                return Err(SessionError::EpochConflict {
                    key: key.clone(),
                    epoch: prefix.epoch,
                });
            }
        };

        self.emit(SessionLifecycleEvent::from_publish_result(
            key.clone(),
            result,
            epoch,
            message_count,
            bytes,
        ));
        Ok(result)
    }

    pub fn get_key(&self, key: &SessionKey) -> Result<Option<SessionPrefix>, SessionError> {
        let now = SystemTime::now();
        let mut guard = self.lock()?;

        let storage_key = key.storage_key();
        if guard.get(&storage_key).is_some()
            && self
                .remove_if_expired(&mut guard, key, &storage_key, now)
                .is_err()
        {
            return Err(SessionError::Expired(key.clone()));
        }

        let Some(entry) = guard.get_mut(&storage_key) else {
            return Ok(None);
        };

        if self.config.lifetime.touch_on_read {
            entry.touch(now);
        }

        Ok(Some(entry.prefix.clone()))
    }

    pub fn delete_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        let mut guard = self.lock()?;
        if guard.remove(&key.storage_key()).is_some() {
            self.emit(SessionLifecycleEvent::SessionDeleted { key: key.clone() });
        }
        Ok(())
    }

    pub fn touch_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        let now = SystemTime::now();
        let mut guard = self.lock()?;
        let storage_key = key.storage_key();

        if guard.get(&storage_key).is_some()
            && self
                .remove_if_expired(&mut guard, key, &storage_key, now)
                .is_err()
        {
            return Err(SessionError::Expired(key.clone()));
        }

        let Some(entry) = guard.get_mut(&storage_key) else {
            return Err(SessionError::NotFound(key.clone()));
        };
        entry.touch(now);
        Ok(())
    }

    pub fn purge_expired(&self) -> Result<usize, SessionError> {
        if !self.config.lifetime.is_enabled() {
            return Ok(0);
        }

        let now = SystemTime::now();
        let mut guard = self.lock()?;
        let expired_keys: Vec<SessionKey> = guard
            .iter()
            .filter(|(_, entry)| {
                session_expired(
                    entry.created_at,
                    entry.last_accessed_at,
                    &self.config.lifetime,
                    now,
                )
            })
            .map(|(storage_key, _)| parse_storage_key(storage_key))
            .collect();

        for key in &expired_keys {
            guard.remove(&key.storage_key());
            self.emit(SessionLifecycleEvent::SessionExpired { key: key.clone() });
        }

        Ok(expired_keys.len())
    }

    pub fn publish(
        &self,
        session_id: &str,
        prefix: SessionPrefix,
    ) -> Result<PublishResult, SessionError> {
        self.publish_key(&SessionKey::default_namespace(session_id), prefix)
    }

    pub fn get(&self, session_id: &str) -> Result<Option<SessionPrefix>, SessionError> {
        self.get_key(&SessionKey::default_namespace(session_id))
    }

    pub fn delete(&self, session_id: &str) -> Result<(), SessionError> {
        self.delete_key(&SessionKey::default_namespace(session_id))
    }
}

fn parse_storage_key(storage_key: &str) -> SessionKey {
    match storage_key.split_once('\0') {
        Some((namespace, session_id)) => SessionKey::new(namespace, session_id),
        None => SessionKey::default_namespace(storage_key),
    }
}

impl SessionStore for MemorySessionStore {
    fn publish_key(
        &self,
        key: &SessionKey,
        prefix: SessionPrefix,
    ) -> Result<PublishResult, SessionError> {
        MemorySessionStore::publish_key(self, key, prefix)
    }

    fn get_key(&self, key: &SessionKey) -> Result<Option<SessionPrefix>, SessionError> {
        MemorySessionStore::get_key(self, key)
    }

    fn delete_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        MemorySessionStore::delete_key(self, key)
    }

    fn touch_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        MemorySessionStore::touch_key(self, key)
    }

    fn purge_expired(&self) -> Result<usize, SessionError> {
        MemorySessionStore::purge_expired(self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::{
        DEFAULT_NAMESPACE, Fingerprint, MemorySessionStore, PublishResult, SessionError,
        SessionKey, SessionLifetime, SessionPrefix, SessionSizeLimits, SessionStore,
        SessionStoreConfig, fingerprints_match,
    };
    use crate::lifecycle::{SessionLifecycleEvent, SessionLifecycleHook};
    use serde_json::{Value, json};

    fn prefix(epoch: u64, content: &str) -> SessionPrefix {
        SessionPrefix {
            epoch,
            messages: vec![json!({"role":"user","content":content})],
            pinned_boundary: None,
            fingerprint: None,
            message_count: None,
        }
    }

    struct RecordingHook {
        events: std::sync::Mutex<Vec<SessionLifecycleEvent>>,
    }

    impl RecordingHook {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: std::sync::Mutex::new(Vec::new()),
            })
        }
    }

    impl SessionLifecycleHook for RecordingHook {
        fn on_event(&self, event: SessionLifecycleEvent) {
            self.events.lock().expect("lock").push(event);
        }
    }

    #[test]
    fn publish_get_delete_round_trip() {
        let store = MemorySessionStore::new();
        assert_eq!(
            store.publish("s1", prefix(2, "hi")).expect("publish"),
            PublishResult::Created
        );
        let got = store.get("s1").expect("get").expect("exists");
        assert_eq!(got.epoch, 2);
        assert_eq!(got.message_count, Some(1));
        store.delete("s1").expect("delete");
        assert!(store.get("s1").expect("get").is_none());
    }

    #[test]
    fn namespace_isolates_same_session_id() {
        let store = MemorySessionStore::new();
        let key_a = SessionKey::new("tenant-a", "s1");
        let key_b = SessionKey::new("tenant-b", "s1");

        store
            .publish_key(&key_a, prefix(1, "a"))
            .expect("publish a");
        store
            .publish_key(&key_b, prefix(1, "b"))
            .expect("publish b");

        assert_eq!(
            store
                .get_key(&key_a)
                .expect("get")
                .expect("exists")
                .messages[0]
                .get("content")
                .and_then(Value::as_str),
            Some("a")
        );
        assert_eq!(
            store
                .get_key(&key_b)
                .expect("get")
                .expect("exists")
                .messages[0]
                .get("content")
                .and_then(Value::as_str),
            Some("b")
        );
    }

    #[test]
    fn default_namespace_compat_matches_bare_session_id() {
        let store = MemorySessionStore::new();
        store.publish("s1", prefix(1, "hi")).expect("publish");
        let key = SessionKey::new(DEFAULT_NAMESPACE, "s1");
        assert!(store.get_key(&key).expect("get").is_some());
    }

    #[test]
    fn epoch_publish_matrix() {
        let store = MemorySessionStore::new();
        assert_eq!(
            store.publish("s1", prefix(1, "v1")).expect("create"),
            PublishResult::Created
        );
        assert_eq!(
            store.publish("s1", prefix(2, "v2")).expect("replace"),
            PublishResult::Replaced
        );
        assert_eq!(
            store.publish("s1", prefix(2, "v2")).expect("idempotent"),
            PublishResult::AlreadyCurrent
        );

        let stale = store.publish("s1", prefix(1, "old"));
        assert!(matches!(
            stale,
            Err(SessionError::StaleEpoch {
                existing_epoch: 2,
                attempted_epoch: 1,
                ..
            })
        ));

        let conflict = store.publish("s1", prefix(2, "different"));
        assert!(matches!(
            conflict,
            Err(SessionError::EpochConflict { epoch: 2, .. })
        ));
    }

    #[test]
    fn concurrent_publish_is_atomic() {
        let store = Arc::new(MemorySessionStore::new());
        let mut handles = Vec::new();

        for _ in 0..32 {
            let store = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                store.publish("s1", prefix(1, "same")).ok()
            }));
        }

        let mut created = 0usize;
        let mut already_current = 0usize;
        for handle in handles {
            match handle.join().expect("join") {
                Some(PublishResult::Created) => created += 1,
                Some(PublishResult::AlreadyCurrent) => already_current += 1,
                Some(PublishResult::Replaced) => {}
                None => {}
            }
        }

        assert_eq!(created, 1);
        assert_eq!(already_current, 31);
        assert_eq!(store.get("s1").expect("get").expect("exists").epoch, 1);
    }

    #[test]
    fn prefix_size_limits_reject_on_publish() {
        let store = MemorySessionStore::with_config(SessionStoreConfig {
            size_limits: SessionSizeLimits {
                max_messages: Some(1),
                max_prefix_bytes: None,
                max_tail_bytes: None,
                max_assembled_bytes: None,
            },
            ..Default::default()
        });
        let key = SessionKey::default_namespace("s1");
        let ok = SessionPrefix {
            epoch: 1,
            messages: vec![json!({"role":"user","content":"one"})],
            pinned_boundary: None,
            fingerprint: None,
            message_count: None,
        };
        store.publish_key(&key, ok).expect("single message ok");

        let too_many = SessionPrefix {
            epoch: 2,
            messages: vec![
                json!({"role":"user","content":"a"}),
                json!({"role":"user","content":"b"}),
            ],
            pinned_boundary: None,
            fingerprint: None,
            message_count: None,
        };
        assert!(matches!(
            store.publish_key(&key, too_many),
            Err(SessionError::PrefixTooLarge {
                limit: 1,
                actual: 2,
                ..
            })
        ));
    }

    #[test]
    fn fingerprint_equality_respects_algorithm() {
        let a = Fingerprint {
            algorithm: "zene-v1".into(),
            value: "abc".into(),
        };
        let b = Fingerprint {
            algorithm: "zene-v1".into(),
            value: "abc".into(),
        };
        let c = Fingerprint {
            algorithm: "other".into(),
            value: "abc".into(),
        };
        assert!(fingerprints_match(&a, &b));
        assert!(!fingerprints_match(&a, &c));
    }

    #[test]
    fn session_store_trait_dispatch() {
        let store: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::new());
        let key = SessionKey::default_namespace("s1");
        assert_eq!(
            store.publish_key(&key, prefix(1, "hi")).expect("publish"),
            PublishResult::Created
        );
        assert!(store.get_key(&key).expect("get").is_some());
    }

    #[test]
    fn idle_ttl_expires_session() {
        let store = MemorySessionStore::with_config(SessionStoreConfig {
            lifetime: SessionLifetime {
                idle_ttl: Some(Duration::from_millis(20)),
                max_lifetime: None,
                touch_on_read: false,
            },
            ..Default::default()
        });
        let key = SessionKey::default_namespace("s1");
        store.publish_key(&key, prefix(1, "hi")).expect("publish");
        thread::sleep(Duration::from_millis(30));
        assert!(matches!(store.get_key(&key), Err(SessionError::Expired(_))));
    }

    #[test]
    fn touch_extends_idle_ttl() {
        let store = MemorySessionStore::with_config(SessionStoreConfig {
            lifetime: SessionLifetime {
                idle_ttl: Some(Duration::from_millis(40)),
                max_lifetime: None,
                touch_on_read: false,
            },
            ..Default::default()
        });
        let key = SessionKey::default_namespace("s1");
        store.publish_key(&key, prefix(1, "hi")).expect("publish");
        thread::sleep(Duration::from_millis(25));
        store.touch_key(&key).expect("touch");
        thread::sleep(Duration::from_millis(25));
        assert!(store.get_key(&key).expect("get").is_some());
    }

    #[test]
    fn purge_expired_removes_sessions() {
        let store = MemorySessionStore::with_config(SessionStoreConfig {
            lifetime: SessionLifetime {
                idle_ttl: Some(Duration::from_millis(10)),
                max_lifetime: None,
                touch_on_read: false,
            },
            ..Default::default()
        });
        store.publish("s1", prefix(1, "hi")).expect("publish");
        thread::sleep(Duration::from_millis(20));
        assert_eq!(store.purge_expired().expect("purge"), 1);
        assert!(store.get("s1").expect("get").is_none());
    }

    #[test]
    fn lifecycle_hook_receives_publish_created() {
        let hook = RecordingHook::new();
        let store = MemorySessionStore::with_config(SessionStoreConfig {
            lifecycle_hook: Some(hook.clone()),
            ..Default::default()
        });
        store.publish("s1", prefix(1, "hi")).expect("publish");
        let events = hook.events.lock().expect("lock");
        assert!(events.iter().any(|event| {
            matches!(
                event,
                SessionLifecycleEvent::PublishCreated {
                    epoch: 1,
                    message_count: 1,
                    ..
                }
            )
        }));
    }
}
