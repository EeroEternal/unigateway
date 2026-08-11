use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redis::{Client, Commands, Script};

use unigateway_session::lifecycle::{SessionLifecycleEvent, SessionLifecycleHook};
use unigateway_session::{
    PublishResult, SessionError, SessionKey, SessionLifetime, SessionPrefix, SessionSizeLimits,
    SessionStore, SessionStoreConfig, is_session_expired, message_json_bytes,
};

const PUBLISH_SCRIPT: &str = r#"
local key = KEYS[1]
local new_epoch = tonumber(ARGV[1])
local prefix_json = ARGV[2]
local created_at = ARGV[3]
local last_accessed_at = ARGV[4]
local idle_ttl = tonumber(ARGV[5])

local existing_epoch = redis.call('HGET', key, 'epoch')
if not existing_epoch then
  redis.call('HSET', key, 'epoch', ARGV[1], 'prefix', prefix_json, 'created_at', created_at, 'last_accessed_at', last_accessed_at)
  if idle_ttl > 0 then redis.call('EXPIRE', key, idle_ttl) end
  return 0
end

local old_epoch = tonumber(existing_epoch)
if new_epoch > old_epoch then
  redis.call('HSET', key, 'epoch', ARGV[1], 'prefix', prefix_json, 'created_at', created_at, 'last_accessed_at', last_accessed_at)
  if idle_ttl > 0 then redis.call('EXPIRE', key, idle_ttl) end
  return 1
elseif new_epoch < old_epoch then
  return -1
else
  local old_prefix = redis.call('HGET', key, 'prefix')
  if old_prefix == prefix_json then
    redis.call('HSET', key, 'last_accessed_at', last_accessed_at)
    if idle_ttl > 0 then redis.call('EXPIRE', key, idle_ttl) end
    return 2
  else
    return -2
  end
end
"#;

/// Configuration for [`RedisSessionStore`].
#[derive(Clone)]
pub struct RedisSessionStoreConfig {
    /// Prefix prepended to every Redis key (namespace isolation is still via [`SessionKey`]).
    pub key_prefix: String,
    pub size_limits: SessionSizeLimits,
    pub lifetime: SessionLifetime,
    pub lifecycle_hook: Option<Arc<dyn SessionLifecycleHook>>,
}

impl Default for RedisSessionStoreConfig {
    fn default() -> Self {
        let defaults = SessionStoreConfig::default();
        Self {
            key_prefix: "unigateway:session:".to_string(),
            size_limits: defaults.size_limits,
            lifetime: defaults.lifetime,
            lifecycle_hook: defaults.lifecycle_hook,
        }
    }
}

impl From<SessionStoreConfig> for RedisSessionStoreConfig {
    fn from(config: SessionStoreConfig) -> Self {
        Self {
            key_prefix: "unigateway:session:".to_string(),
            size_limits: config.size_limits,
            lifetime: config.lifetime,
            lifecycle_hook: config.lifecycle_hook,
        }
    }
}

/// Redis hash-backed session prefix store with epoch CAS via Lua.
pub struct RedisSessionStore {
    client: Client,
    config: RedisSessionStoreConfig,
    publish_script: Script,
}

impl RedisSessionStore {
    pub fn open(redis_url: &str) -> Result<Self, SessionError> {
        Self::with_config(redis_url, RedisSessionStoreConfig::default())
    }

    pub fn with_config(
        redis_url: &str,
        config: RedisSessionStoreConfig,
    ) -> Result<Self, SessionError> {
        let client = Client::open(redis_url)
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        Ok(Self {
            client,
            config,
            publish_script: Script::new(PUBLISH_SCRIPT),
        })
    }

    fn connection(&self) -> Result<redis::Connection, SessionError> {
        self.client
            .get_connection()
            .map_err(|error| SessionError::Unavailable(error.to_string()))
    }

    fn redis_key(&self, key: &SessionKey) -> String {
        format!("{}{}", self.config.key_prefix, key.storage_key())
    }

    fn parse_session_key(&self, redis_key: &str) -> SessionKey {
        let storage_key = redis_key
            .strip_prefix(&self.config.key_prefix)
            .unwrap_or(redis_key);
        match storage_key.split_once('\0') {
            Some((namespace, session_id)) => SessionKey::new(namespace, session_id),
            None => SessionKey::default_namespace(storage_key),
        }
    }

    fn emit(&self, event: SessionLifecycleEvent) {
        if let Some(hook) = &self.config.lifecycle_hook {
            hook.on_event(event);
        }
    }

    fn idle_ttl_secs(&self) -> u64 {
        self.config
            .lifetime
            .idle_ttl
            .map(|duration| duration.as_secs().max(1))
            .unwrap_or(0)
    }

    fn refresh_idle_ttl(
        &self,
        conn: &mut redis::Connection,
        redis_key: &str,
    ) -> Result<(), SessionError> {
        let idle_ttl = self.idle_ttl_secs();
        if idle_ttl > 0 {
            let _: () = conn
                .expire(redis_key, idle_ttl as i64)
                .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        }
        Ok(())
    }

    fn load_record(
        &self,
        conn: &mut redis::Connection,
        redis_key: &str,
    ) -> Result<Option<StoredRecord>, SessionError> {
        let map: Vec<(String, String)> = conn
            .hgetall(redis_key)
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        if map.is_empty() {
            return Ok(None);
        }

        let mut epoch: Option<u64> = None;
        let mut prefix_json = None;
        let mut created_at_secs = None;
        let mut last_accessed_at_secs = None;

        for (field, value) in map {
            match field.as_str() {
                "epoch" => epoch = value.parse().ok(),
                "prefix" => prefix_json = Some(value),
                "created_at" => created_at_secs = value.parse().ok(),
                "last_accessed_at" => last_accessed_at_secs = value.parse().ok(),
                _ => {}
            }
        }

        let (Some(epoch), Some(prefix_json), Some(created_at_secs), Some(last_accessed_at_secs)) =
            (epoch, prefix_json, created_at_secs, last_accessed_at_secs)
        else {
            return Err(SessionError::Unavailable(format!(
                "corrupt session record at {redis_key}"
            )));
        };

        let prefix: SessionPrefix = serde_json::from_str(&prefix_json)
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        if prefix.epoch != epoch {
            return Err(SessionError::Unavailable(format!(
                "epoch mismatch in stored prefix at {redis_key}"
            )));
        }

        Ok(Some(StoredRecord {
            prefix,
            created_at: secs_to_system_time(created_at_secs),
            last_accessed_at: secs_to_system_time(last_accessed_at_secs),
        }))
    }

    fn delete_record(
        &self,
        conn: &mut redis::Connection,
        redis_key: &str,
    ) -> Result<(), SessionError> {
        conn.del(redis_key)
            .map(|_: ()| ())
            .map_err(|error| SessionError::Unavailable(error.to_string()))
    }

    fn touch_record(
        &self,
        conn: &mut redis::Connection,
        redis_key: &str,
        now: SystemTime,
    ) -> Result<(), SessionError> {
        let last_accessed_at = system_time_to_secs(now);
        let _: () = conn
            .hset(redis_key, "last_accessed_at", last_accessed_at.to_string())
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        self.refresh_idle_ttl(conn, redis_key)
    }

    fn remove_if_expired(
        &self,
        conn: &mut redis::Connection,
        key: &SessionKey,
        redis_key: &str,
        now: SystemTime,
    ) -> Result<Option<StoredRecord>, SessionError> {
        let Some(record) = self.load_record(conn, redis_key)? else {
            return Ok(None);
        };

        if is_session_expired(
            record.created_at,
            record.last_accessed_at,
            &self.config.lifetime,
            now,
        ) {
            self.delete_record(conn, redis_key)?;
            self.emit(SessionLifecycleEvent::SessionExpired { key: key.clone() });
            return Err(SessionError::Expired(key.clone()));
        }

        Ok(Some(record))
    }
}

struct StoredRecord {
    prefix: SessionPrefix,
    created_at: SystemTime,
    last_accessed_at: SystemTime,
}

impl SessionStore for RedisSessionStore {
    fn publish_key(
        &self,
        key: &SessionKey,
        prefix: SessionPrefix,
    ) -> Result<PublishResult, SessionError> {
        let prefix = prefix.normalize();
        self.config
            .size_limits
            .validate_prefix(key, &prefix.messages)?;

        let message_count = prefix.message_count.unwrap_or(prefix.messages.len() as u64);
        let bytes = message_json_bytes(&prefix.messages)?;
        let epoch = prefix.epoch;
        let now = SystemTime::now();
        let redis_key = self.redis_key(key);
        let prefix_json = serde_json::to_string(&prefix)
            .map_err(|error| SessionError::InvalidContext(error.to_string()))?;

        let mut conn = self.connection()?;

        if let Err(error) = self.remove_if_expired(&mut conn, key, &redis_key, now)
            && !matches!(error, SessionError::Expired(_))
        {
            return Err(error);
        }

        let code: i32 = self
            .publish_script
            .key(&redis_key)
            .arg(epoch.to_string())
            .arg(prefix_json)
            .arg(system_time_to_secs(now).to_string())
            .arg(system_time_to_secs(now).to_string())
            .arg(self.idle_ttl_secs().to_string())
            .invoke(&mut conn)
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;

        let result = match code {
            0 => PublishResult::Created,
            1 => PublishResult::Replaced,
            2 => PublishResult::AlreadyCurrent,
            -1 => {
                let existing_epoch = self
                    .load_record(&mut conn, &redis_key)?
                    .map(|record| record.prefix.epoch)
                    .unwrap_or(epoch);
                self.emit(SessionLifecycleEvent::StalePublish {
                    key: key.clone(),
                    existing_epoch,
                    attempted_epoch: prefix.epoch,
                });
                return Err(SessionError::StaleEpoch {
                    key: key.clone(),
                    existing_epoch,
                    attempted_epoch: prefix.epoch,
                });
            }
            -2 => {
                self.emit(SessionLifecycleEvent::EpochConflict {
                    key: key.clone(),
                    epoch: prefix.epoch,
                });
                return Err(SessionError::EpochConflict {
                    key: key.clone(),
                    epoch: prefix.epoch,
                });
            }
            other => {
                return Err(SessionError::Unavailable(format!(
                    "unexpected publish script result: {other}"
                )));
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

    fn get_key(&self, key: &SessionKey) -> Result<Option<SessionPrefix>, SessionError> {
        let now = SystemTime::now();
        let redis_key = self.redis_key(key);
        let mut conn = self.connection()?;

        let record = match self.remove_if_expired(&mut conn, key, &redis_key, now) {
            Ok(record) => record,
            Err(SessionError::Expired(_)) => return Err(SessionError::Expired(key.clone())),
            Err(error) => return Err(error),
        };

        let Some(record) = record else {
            return Ok(None);
        };

        if self.config.lifetime.touch_on_read {
            self.touch_record(&mut conn, &redis_key, now)?;
        }

        Ok(Some(record.prefix))
    }

    fn delete_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        let redis_key = self.redis_key(key);
        let mut conn = self.connection()?;
        let removed: i32 = conn
            .del(&redis_key)
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        if removed > 0 {
            self.emit(SessionLifecycleEvent::SessionDeleted { key: key.clone() });
        }
        Ok(())
    }

    fn touch_key(&self, key: &SessionKey) -> Result<(), SessionError> {
        let now = SystemTime::now();
        let redis_key = self.redis_key(key);
        let mut conn = self.connection()?;

        if self
            .remove_if_expired(&mut conn, key, &redis_key, now)
            .is_err()
        {
            return Err(SessionError::Expired(key.clone()));
        }

        let exists: bool = conn
            .hexists(&redis_key, "epoch")
            .map_err(|error| SessionError::Unavailable(error.to_string()))?;
        if !exists {
            return Err(SessionError::NotFound(key.clone()));
        }

        self.touch_record(&mut conn, &redis_key, now)
    }

    fn purge_expired(&self) -> Result<usize, SessionError> {
        if !self.config.lifetime.is_enabled() {
            return Ok(0);
        }

        let now = SystemTime::now();
        let pattern = format!("{}*", self.config.key_prefix);
        let mut conn = self.connection()?;
        let mut cursor = 0u64;
        let mut purged = 0usize;

        loop {
            let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query(&mut conn)
                .map_err(|error| SessionError::Unavailable(error.to_string()))?;

            for redis_key in keys {
                let session_key = self.parse_session_key(&redis_key);
                if let Some(record) = self.load_record(&mut conn, &redis_key)?
                    && is_session_expired(
                        record.created_at,
                        record.last_accessed_at,
                        &self.config.lifetime,
                        now,
                    )
                {
                    self.delete_record(&mut conn, &redis_key)?;
                    self.emit(SessionLifecycleEvent::SessionExpired { key: session_key });
                    purged += 1;
                }
            }

            if next_cursor == 0 {
                break;
            }
            cursor = next_cursor;
        }

        Ok(purged)
    }
}

fn system_time_to_secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn secs_to_system_time(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use serde_json::json;
    use unigateway_session::{PublishResult, SessionError, SessionKey, SessionLifetime};

    use super::*;

    fn prefix(epoch: u64, content: &str) -> SessionPrefix {
        SessionPrefix {
            epoch,
            messages: vec![json!({"role":"user","content":content})],
            pinned_boundary: None,
            fingerprint: None,
            message_count: None,
        }
    }

    fn redis_url() -> Option<String> {
        std::env::var("REDIS_URL").ok()
    }

    fn open_store(config: RedisSessionStoreConfig) -> Option<RedisSessionStore> {
        let url = redis_url()?;
        Some(RedisSessionStore::with_config(&url, config).expect("connect"))
    }

    fn unique_key(label: &str) -> SessionKey {
        let id = format!(
            "{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        SessionKey::new("redis-test", id)
    }

    #[test]
    #[ignore = "requires REDIS_URL and a running Redis server"]
    fn publish_get_delete_round_trip() {
        let Some(store) = open_store(RedisSessionStoreConfig::default()) else {
            return;
        };
        let key = unique_key("round-trip");
        assert_eq!(
            store.publish_key(&key, prefix(1, "hi")).expect("publish"),
            PublishResult::Created
        );
        let got = store.get_key(&key).expect("get").expect("exists");
        assert_eq!(got.epoch, 1);
        store.delete_key(&key).expect("delete");
        assert!(store.get_key(&key).expect("get").is_none());
    }

    #[test]
    #[ignore = "requires REDIS_URL and a running Redis server"]
    fn epoch_publish_matrix() {
        let Some(store) = open_store(RedisSessionStoreConfig::default()) else {
            return;
        };
        let key = unique_key("epoch-matrix");
        assert_eq!(
            store.publish_key(&key, prefix(1, "v1")).expect("create"),
            PublishResult::Created
        );
        assert_eq!(
            store.publish_key(&key, prefix(2, "v2")).expect("replace"),
            PublishResult::Replaced
        );
        assert_eq!(
            store
                .publish_key(&key, prefix(2, "v2"))
                .expect("idempotent"),
            PublishResult::AlreadyCurrent
        );
        assert!(matches!(
            store.publish_key(&key, prefix(1, "old")),
            Err(SessionError::StaleEpoch { .. })
        ));
        assert!(matches!(
            store.publish_key(&key, prefix(2, "different")),
            Err(SessionError::EpochConflict { .. })
        ));
        store.delete_key(&key).expect("cleanup");
    }

    #[test]
    #[ignore = "requires REDIS_URL and a running Redis server"]
    fn namespace_isolates_same_session_id() {
        let Some(store) = open_store(RedisSessionStoreConfig::default()) else {
            return;
        };
        let session_id = format!(
            "shared-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let key_a = SessionKey::new("tenant-a", &session_id);
        let key_b = SessionKey::new("tenant-b", &session_id);
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
                .and_then(|value| value.as_str()),
            Some("a")
        );
        store.delete_key(&key_a).expect("cleanup a");
        store.delete_key(&key_b).expect("cleanup b");
    }

    #[test]
    #[ignore = "requires REDIS_URL and a running Redis server"]
    fn idle_ttl_expires_session() {
        let Some(store) = open_store(RedisSessionStoreConfig {
            lifetime: SessionLifetime {
                idle_ttl: Some(Duration::from_secs(1)),
                max_lifetime: None,
                touch_on_read: false,
            },
            ..Default::default()
        }) else {
            return;
        };
        let key = unique_key("idle-ttl");
        store.publish_key(&key, prefix(1, "hi")).expect("publish");
        thread::sleep(Duration::from_secs(2));
        assert!(matches!(store.get_key(&key), Err(SessionError::Expired(_))));
    }

    #[test]
    #[ignore = "requires REDIS_URL and a running Redis server"]
    fn session_store_trait_object() {
        let Some(store) = open_store(RedisSessionStoreConfig::default()) else {
            return;
        };
        let store: Arc<dyn SessionStore> = Arc::new(store);
        let key = unique_key("trait-object");
        assert_eq!(
            store.publish_key(&key, prefix(1, "hi")).expect("publish"),
            PublishResult::Created
        );
        store.delete_key(&key).expect("cleanup");
    }
}
