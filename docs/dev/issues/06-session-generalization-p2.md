# Session generalization P2: TTL, touch, purge, lifecycle hooks

## Summary

Add **production lifecycle capabilities** to `unigateway-session`: idle and absolute TTL with lazy expiration, explicit `touch` / `purge_expired`, and optional lifecycle hooks for observability.

Depends on: [`04-session-generalization-p0.md`](04-session-generalization-p0.md), [`05-session-generalization-p1.md`](05-session-generalization-p1.md).

External Redis/Postgres store crates remain out of scope.

## P2 scope

### P2-1 — Session lifetime config

```rust
pub struct SessionLifetime {
    pub idle_ttl: Option<Duration>,
    pub max_lifetime: Option<Duration>,
    pub touch_on_read: bool,  // default: true
}
```

- Default: all TTL disabled (unchanged behavior).
- `publish` success always refreshes idle access time.
- `max_lifetime` measured from session creation; reset on epoch replace.
- Expired sessions return `SessionError::Expired` (hosts may map to `NotFound`).

### P2-2 — Lazy expiration

- Check TTL on `get_key` and `publish_key` (before CAS).
- Remove expired entries eagerly on access.
- `purge_expired()` for optional background sweeps; returns removed count.

### P2-3 — Touch

- `touch_key(&SessionKey)` refreshes idle timestamp when session exists and is not expired.
- Middleware calls `touch_key` after successful delta assembly when `touch_on_delta` is true (default).

### P2-4 — Lifecycle hooks

- `SessionLifecycleEvent` enum (no message content).
- `SessionLifecycleHook` trait; optional on store and middleware config.
- Events: publish created/replaced/idempotent, stale/conflict, delta hit/miss, expired, deleted, fingerprint/tail/size rejections, store unavailable.

## Non-goals

- Background timer tasks inside the crate (hosts call `purge_expired` on their own schedule).
- Redis/Postgres implementations.
- Logging prompts or credentials in events.

## Acceptance criteria

- [x] Idle TTL expires inactive sessions.
- [x] Max lifetime expires regardless of recent touches.
- [x] Publish refreshes idle timer; epoch replace resets creation time.
- [x] Delta success optionally touches session (default on).
- [x] `full` delivery does not read or touch session.
- [x] Expired session returns `Expired`, not silent corruption.
- [x] `purge_expired` removes all expired entries.
- [x] Lifecycle hook receives events without message bodies.
- [x] Default config: no TTL (backward compatible).

## References

- SmartGate review: expired → host maps to `404 SESSION_NOT_FOUND`
- P1 spec: [`05-session-generalization-p1.md`](05-session-generalization-p1.md)
