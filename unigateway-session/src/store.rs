use std::collections::HashMap;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stored conversation prefix for delta assembly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionPrefix {
    pub epoch: u64,
    pub messages: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_boundary: Option<u64>,
}

#[derive(Debug)]
pub enum SessionStoreError {
    NotFound(String),
    Poisoned,
}

impl std::fmt::Display for SessionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "session not found: {id}"),
            Self::Poisoned => write!(f, "store lock poisoned"),
        }
    }
}

impl std::error::Error for SessionStoreError {}

/// In-memory session prefix store (reference implementation).
#[derive(Default)]
pub struct MemorySessionStore {
    inner: RwLock<HashMap<String, SessionPrefix>>,
}

impl MemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(
        &self,
        session_id: &str,
        prefix: SessionPrefix,
    ) -> Result<(), SessionStoreError> {
        self.inner
            .write()
            .map_err(|_| SessionStoreError::Poisoned)?
            .insert(session_id.to_string(), prefix);
        Ok(())
    }

    pub fn get(&self, session_id: &str) -> Result<Option<SessionPrefix>, SessionStoreError> {
        Ok(self
            .inner
            .read()
            .map_err(|_| SessionStoreError::Poisoned)?
            .get(session_id)
            .cloned())
    }

    pub fn delete(&self, session_id: &str) -> Result<(), SessionStoreError> {
        self.inner
            .write()
            .map_err(|_| SessionStoreError::Poisoned)?
            .remove(session_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{MemorySessionStore, SessionPrefix};
    use serde_json::json;

    #[test]
    fn publish_get_delete_round_trip() {
        let store = MemorySessionStore::new();
        store
            .publish(
                "s1",
                SessionPrefix {
                    epoch: 2,
                    messages: vec![json!({"role":"user","content":"hi"})],
                    pinned_boundary: None,
                },
            )
            .expect("publish");
        let got = store.get("s1").expect("get").expect("exists");
        assert_eq!(got.epoch, 2);
        store.delete("s1").expect("delete");
        assert!(store.get("s1").expect("get").is_none());
    }
}
