//! Redis-backed [`SessionStore`](unigateway_session::SessionStore) for multi-instance embedders.
//!
//! Requires a running Redis server. Use [`RedisSessionStore::open`] with a standard Redis URL
//! (`redis://127.0.0.1/`). Integration tests are gated on the `REDIS_URL` environment variable.

mod store;

pub use store::{RedisSessionStore, RedisSessionStoreConfig};
