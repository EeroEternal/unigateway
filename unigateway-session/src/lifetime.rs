use std::time::{Duration, SystemTime};

/// Session lifetime policy for reference store implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLifetime {
    /// Expire when idle longer than this duration.
    pub idle_ttl: Option<Duration>,
    /// Expire when older than this duration since creation.
    pub max_lifetime: Option<Duration>,
    /// Refresh idle TTL on successful reads (`get_key` / `touch_key`).
    pub touch_on_read: bool,
}

impl Default for SessionLifetime {
    fn default() -> Self {
        Self {
            idle_ttl: None,
            max_lifetime: None,
            touch_on_read: true,
        }
    }
}

impl SessionLifetime {
    pub fn is_enabled(&self) -> bool {
        self.idle_ttl.is_some() || self.max_lifetime.is_some()
    }
}

pub(crate) fn session_expired(
    created_at: SystemTime,
    last_accessed_at: SystemTime,
    lifetime: &SessionLifetime,
    now: SystemTime,
) -> bool {
    if let Some(max_lifetime) = lifetime.max_lifetime
        && duration_since(created_at, now).is_some_and(|elapsed| elapsed > max_lifetime)
    {
        return true;
    }

    if let Some(idle_ttl) = lifetime.idle_ttl
        && duration_since(last_accessed_at, now).is_some_and(|elapsed| elapsed > idle_ttl)
    {
        return true;
    }

    false
}

fn duration_since(from: SystemTime, now: SystemTime) -> Option<Duration> {
    now.duration_since(from).ok()
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::{SessionLifetime, session_expired};

    fn t(secs: u64) -> std::time::SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn idle_ttl_expires_after_last_access() {
        let lifetime = SessionLifetime {
            idle_ttl: Some(Duration::from_secs(60)),
            max_lifetime: None,
            touch_on_read: true,
        };
        assert!(!session_expired(t(0), t(100), &lifetime, t(150)));
        assert!(session_expired(t(0), t(100), &lifetime, t(161)));
    }

    #[test]
    fn max_lifetime_expires_even_when_recently_touched() {
        let lifetime = SessionLifetime {
            idle_ttl: Some(Duration::from_secs(3600)),
            max_lifetime: Some(Duration::from_secs(300)),
            touch_on_read: true,
        };
        assert!(!session_expired(t(0), t(250), &lifetime, t(250)));
        assert!(session_expired(t(0), t(250), &lifetime, t(301)));
    }
}
