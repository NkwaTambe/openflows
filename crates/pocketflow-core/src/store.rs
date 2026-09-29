// crates/pocketflow-core/src/store.rs
//
// SharedStore — dual-backend (in-memory for dev, Redis for production).
// Same interface regardless of backend. Swap via REDIS_URL env var.

use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::{debug, trace};

// ── Event ring buffer ─────────────────────────────────────────────────────

const RING_BUFFER_SIZE: usize = 1000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoreEvent {
    pub agent: String,
    pub event_type: String,
    pub payload: Value,
    pub ts: u64, // unix millis
}

// ── In-memory backend ─────────────────────────────────────────────────────

struct InMemoryBackend {
    map: RwLock<HashMap<String, Value>>,
}

impl InMemoryBackend {
    fn new() -> Self {
        Self {
            map: RwLock::new(HashMap::new()),
        }
    }

    async fn keys(&self, pattern: &str) -> Vec<String> {
        let map = self.map.read().await;
        map.keys()
            .filter(|k| {
                if pattern == "*" || pattern.ends_with('*') {
                    let prefix = pattern.trim_end_matches('*');
                    k.starts_with(prefix)
                } else {
                    k == &pattern
                }
            })
            .cloned()
            .collect()
    }
}

// ── Redis backend ─────────────────────────────────────────────────────────
struct RedisBackend {
    client: fred::clients::Client,
}

impl RedisBackend {
    async fn new(url: &str) -> Result<Self> {
        use fred::prelude::*;
        let config = Config::from_url(url)?;
        let client = Builder::from_config(config).build()?;
        client.init().await?;
        Ok(Self { client })
    }

    async fn keys(&self, pattern: &str) -> Vec<String> {
        use fred::types::scan::Scanner;
        use futures::StreamExt;
        let mut keys = Vec::new();
        let mut stream = self.client.scan(pattern, None, None);
        while let Some(result) = stream.next().await {
            if let Ok(mut scan_result) = result {
                if let Some(page) = scan_result.take_results() {
                    for key in page {
                        if let Some(s) = key.into_string() {
                            keys.push(s);
                        }
                    }
                }
                if scan_result.has_more() {
                    scan_result.next();
                }
            }
        }
        keys
    }

    async fn ping(&self) -> Result<()> {
        use fred::prelude::*;
        let _: String = self.client.ping(None).await?;
        Ok(())
    }
}

// ── Backend enum ──────────────────────────────────────────────────────────

#[derive(Clone)]
enum Backend {
    InMemory(Arc<InMemoryBackend>),
    Redis(Arc<RedisBackend>),
}

impl Backend {
    async fn get(&self, key: &str) -> Option<Value> {
        match self {
            Backend::InMemory(b) => b.map.read().await.get(key).cloned(),
            Backend::Redis(b) => {
                use fred::prelude::*;
                let raw: Option<String> = b.client.get(key).await.ok()?;
                raw.and_then(|s| serde_json::from_str(&s).ok())
            }
        }
    }

    async fn set(&self, key: &str, value: Value) {
        match self {
            Backend::InMemory(b) => {
                b.map.write().await.insert(key.to_string(), value);
            }
            Backend::Redis(b) => {
                use fred::prelude::*;
                if let Ok(s) = serde_json::to_string(&value) {
                    let _: core::result::Result<(), _> =
                        b.client.set::<(), _, _>(key, s, None, None, false).await;
                }
            }
        }
    }

    async fn del(&self, key: &str) {
        match self {
            Backend::InMemory(b) => {
                b.map.write().await.remove(key);
            }
            Backend::Redis(b) => {
                use fred::prelude::*;
                let _: core::result::Result<i64, _> = b.client.del(key).await;
            }
        }
    }

    async fn keys(&self, pattern: &str) -> Vec<String> {
        match self {
            Backend::InMemory(b) => b.keys(pattern).await,
            Backend::Redis(b) => b.keys(pattern).await,
        }
    }

    async fn ping(&self) -> Result<()> {
        match self {
            Backend::InMemory(_) => Ok(()),
            Backend::Redis(b) => b.ping().await,
        }
    }

    /// Error-propagating get. Unlike [`get`](Self::get) this does not swallow a
    /// backing-store failure, so callers that must fail closed (or verify a
    /// write) can distinguish "absent" from "unreadable".
    async fn get_result(&self, key: &str) -> Result<Option<Value>> {
        match self {
            Backend::InMemory(b) => Ok(b.map.read().await.get(key).cloned()),
            Backend::Redis(b) => {
                use fred::prelude::*;
                let raw: Option<String> = b.client.get(key).await?;
                match raw {
                    Some(s) => Ok(Some(serde_json::from_str(&s)?)),
                    None => Ok(None),
                }
            }
        }
    }

    /// Error-propagating set. Unlike [`set`](Self::set) this surfaces backing-store
    /// failures instead of discarding them.
    async fn set_result(&self, key: &str, value: Value) -> Result<()> {
        match self {
            Backend::InMemory(b) => {
                b.map.write().await.insert(key.to_string(), value);
                Ok(())
            }
            Backend::Redis(b) => {
                use fred::prelude::*;
                let s = serde_json::to_string(&value)?;
                b.client.set::<(), _, _>(key, s, None, None, false).await?;
                Ok(())
            }
        }
    }

    /// Error-propagating key scan. Unlike [`keys`](Self::keys) this surfaces a
    /// backing-store scan failure instead of returning a possibly-empty list,
    /// so callers do not mistake an outage for "nothing matches".
    async fn keys_result(&self, pattern: &str) -> Result<Vec<String>> {
        match self {
            Backend::InMemory(b) => Ok(b.keys(pattern).await),
            Backend::Redis(b) => {
                use fred::types::scan::Scanner;
                use futures::StreamExt;
                let mut keys = Vec::new();
                let mut stream = b.client.scan(pattern, None, None);
                while let Some(result) = stream.next().await {
                    let mut scan_result = result?;
                    if let Some(page) = scan_result.take_results() {
                        for key in page {
                            if let Some(s) = key.into_string() {
                                keys.push(s);
                            }
                        }
                    }
                    if scan_result.has_more() {
                        scan_result.next();
                    }
                }
                Ok(keys)
            }
        }
    }

    /// Atomically set `key` to `value` only if `key` does not already exist.
    /// Returns `true` if the key was set, `false` if it already existed.
    async fn set_if_absent(&self, key: &str, value: Value) -> Result<bool> {
        match self {
            Backend::InMemory(b) => {
                let mut map = b.map.write().await;
                if map.contains_key(key) {
                    Ok(false)
                } else {
                    map.insert(key.to_string(), value);
                    Ok(true)
                }
            }
            Backend::Redis(b) => {
                use fred::prelude::*;
                let s = serde_json::to_string(&value)?;
                let set: bool = b.client.setnx(key, s).await?;
                Ok(set)
            }
        }
    }

    /// Atomically append a JSON value to a JSON-array value stored at `key`.
    ///
    /// The read-modify-write happens atomically in the backing store (a Lua
    /// script on Redis, a lock on the in-memory backend), so concurrent appends
    /// cannot lose each other. Returns the new array length. Fails (rather than
    /// overwriting) if the stored value is present but is not a JSON array.
    async fn append(&self, key: &str, value: Value) -> Result<u64> {
        match self {
            Backend::InMemory(b) => {
                let mut map = b.map.write().await;
                let mut arr: Vec<Value> = match map.get(key).cloned() {
                    None => Vec::new(),
                    Some(existing) => serde_json::from_value(existing)
                        .map_err(|e| anyhow::anyhow!("key {key} is not a JSON array: {e}"))?,
                };
                arr.push(value);
                let len = arr.len() as u64;
                map.insert(key.to_string(), serde_json::to_value(arr)?);
                Ok(len)
            }
            Backend::Redis(b) => {
                use fred::prelude::*;
                // Lua script: GET the array, decode, append, SET, return length.
                // Runs atomically in Redis, protecting the array from concurrent
                // read-modify-write (including the controller's issue sync).
                let script = r#"
                    local raw = redis.call('GET', KEYS[1])
                    local arr = {}
                    if raw then
                        arr = cjson.decode(raw)
                        if type(arr) ~= 'table' then
                            return redis.error_reply('expected a JSON array')
                        end
                    end
                    table.insert(arr, cjson.decode(ARGV[1]))
                    redis.call('SET', KEYS[1], cjson.encode(arr))
                    return #arr
                "#;
                let len: i64 = b
                    .client
                    .eval::<i64, _, _, _>(script, vec![key.to_string()], vec![value.to_string()])
                    .await?;
                Ok(len as u64)
            }
        }
    }

    /// Atomically increment a counter and return the new value (INCR semantics).
    async fn incr(&self, key: &str) -> Result<u64> {
        match self {
            Backend::InMemory(b) => {
                let mut map = b.map.write().await;
                let next = map
                    .get(key)
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    .saturating_add(1);
                map.insert(key.to_string(), serde_json::json!(next));
                Ok(next)
            }
            Backend::Redis(b) => {
                use fred::prelude::*;
                let n: i64 = b.client.incr(key).await?;
                Ok(n as u64)
            }
        }
    }
}

// ── SharedStore (public API) ──────────────────────────────────────────────

#[derive(Clone)]
pub struct SharedStore {
    backend: Backend,
    ring_buffer: Arc<RwLock<Vec<StoreEvent>>>,
    tenant: String,
}

impl SharedStore {
    /// In-memory backend — use for dev and tests.
    pub fn new_in_memory() -> Self {
        Self::new_in_memory_with_tenant("default")
    }

    /// In-memory backend with explicit tenant — for testing multi-tenancy.
    pub fn new_in_memory_with_tenant(tenant: impl Into<String>) -> Self {
        Self {
            backend: Backend::InMemory(Arc::new(InMemoryBackend::new())),
            ring_buffer: Arc::new(RwLock::new(Vec::with_capacity(RING_BUFFER_SIZE))),
            tenant: tenant.into(),
        }
    }

    /// Redis backend — use for Docker Compose and production.
    /// Tenant is derived from the OPENFLOWS_TENANT env var, or "default" if unset.
    /// This ensures all keys are namespaced as `ns:{tenant}:*` for tenant isolation.
    pub async fn new_redis(url: &str) -> Result<Self> {
        Self::new_redis_with_tenant(url, None).await
    }

    /// Redis backend with explicit or derived tenant.
    /// If tenant is None, reads from OPENFLOWS_TENANT env var.
    pub async fn new_redis_with_tenant(url: &str, tenant: Option<String>) -> Result<Self> {
        let resolved_tenant = if let Some(t) = tenant {
            t
        } else {
            config::EnvConfig::from_env()
                .map(|e| e.tenant.effective_tenant().to_string())
                .unwrap_or_else(|_| "default".to_string())
        };

        Ok(Self {
            backend: Backend::Redis(Arc::new(RedisBackend::new(url).await?)),
            ring_buffer: Arc::new(RwLock::new(Vec::with_capacity(RING_BUFFER_SIZE))),
            tenant: resolved_tenant,
        })
    }

    /// Build a tenant-namespaced key: `ns:{tenant}:{key}`.
    fn ns_key(&self, key: &str) -> String {
        format!("ns:{}:{}", self.tenant, key)
    }

    // ── Core get/set/del ─────────────────────────────────────────────

    pub async fn get(&self, key: &str) -> Option<Value> {
        let ns_key = self.ns_key(key);
        let v = self.backend.get(&ns_key).await;
        trace!(key = %ns_key, found = v.is_some(), "store.get");
        v
    }

    pub async fn set(&self, key: &str, value: Value) {
        let ns_key = self.ns_key(key);
        debug!(key = %ns_key, "store.set");
        self.backend.set(&ns_key, value).await;
    }

    pub async fn del(&self, key: &str) {
        let ns_key = self.ns_key(key);
        debug!(key = %ns_key, "store.del");
        self.backend.del(&ns_key).await;
    }

    pub async fn keys(&self, pattern: &str) -> Vec<String> {
        // For pattern matching, we need to handle both the namespace prefix
        // and the fact that SCAN returns full keys. The pattern should match
        // against the namespaced form: ns:{tenant}:{pattern}
        let ns_pattern = self.ns_key(pattern);
        self.backend.keys(&ns_pattern).await
    }

    /// Raw key scan on the backend WITHOUT tenant namespacing.
    /// `pattern` is matched as-is against full Redis keys (e.g. "ns:*").
    /// Returns the full Redis keys that matched.
    pub async fn raw_keys(&self, pattern: &str) -> Vec<String> {
        self.backend.keys(pattern).await
    }

    /// Raw get of a full key WITHOUT tenant namespacing.
    /// Use this with fully-qualified keys (e.g. `ns:{tenant}:...`) when the
    /// caller manages namespacing itself (as the multi-tenant manager does).
    pub async fn raw_get(&self, key: &str) -> Option<Value> {
        self.backend.get(key).await
    }

    /// Error-propagating, tenant-scoped get. See [`get_result`](Self::get_result).
    pub async fn get_result(&self, key: &str) -> Result<Option<Value>> {
        let ns_key = self.ns_key(key);
        self.backend.get_result(&ns_key).await
    }

    /// Error-propagating raw get (no tenant namespacing).
    pub async fn raw_get_result(&self, key: &str) -> Result<Option<Value>> {
        self.backend.get_result(key).await
    }

    /// Error-propagating raw set (no tenant namespacing).
    pub async fn raw_set_result(&self, key: &str, value: Value) -> Result<()> {
        self.backend.set_result(key, value).await
    }

    /// Error-propagating raw key scan (no tenant namespacing).
    pub async fn raw_keys_result(&self, pattern: &str) -> Result<Vec<String>> {
        self.backend.keys_result(pattern).await
    }

    /// Atomically set a full key only if it does not exist (SETNX semantics).
    pub async fn raw_set_if_absent(&self, key: &str, value: Value) -> Result<bool> {
        self.backend.set_if_absent(key, value).await
    }

    /// Atomically append a value to a JSON-array value at a full key.
    pub async fn raw_append(&self, key: &str, value: Value) -> Result<u64> {
        self.backend.append(key, value).await
    }

    /// Atomically increment a counter at a full key and return the new value.
    pub async fn raw_incr(&self, key: &str) -> Result<u64> {
        self.backend.incr(key).await
    }

    /// Raw set of a full key WITHOUT tenant namespacing.
    pub async fn raw_set(&self, key: &str, value: Value) {
        self.backend.set(key, value).await;
    }

    /// Raw delete of a full Redis key WITHOUT tenant namespacing.
    /// Use this with keys returned by `keys()` / `raw_keys()`, which are
    /// already fully-qualified and must not be re-prefixed.
    pub async fn raw_del(&self, key: &str) {
        self.backend.del(key).await;
    }

    /// Check whether the underlying store backend is reachable.
    pub async fn ping(&self) -> Result<()> {
        self.backend.ping().await
    }

    /// Typed get — deserialises JSON into T. Returns None on missing key or type mismatch.
    pub async fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let v = self.get(key).await?;
        serde_json::from_value(v).ok()
    }

    /// Typed set — serialises T to JSON Value.
    pub async fn set_typed<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let v = serde_json::to_value(value)?;
        self.set(key, v).await;
        Ok(())
    }

    // ── Event ring buffer ─────────────────────────────────────────────

    /// Emit a structured event. Every node lifecycle phase should call this.
    pub async fn emit(&self, agent: &str, event_type: &str, payload: Value) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = StoreEvent {
            agent: agent.to_string(),
            event_type: event_type.to_string(),
            payload,
            ts,
        };

        let mut buf = self.ring_buffer.write().await;
        if buf.len() >= RING_BUFFER_SIZE {
            buf.remove(0); // drop oldest
        }
        buf.push(event);
    }

    /// Returns all events since `cursor` (index). Used by the TUI tail loop.
    pub async fn get_events_since(&self, cursor: usize) -> Vec<StoreEvent> {
        let buf = self.ring_buffer.read().await;
        if cursor >= buf.len() {
            return vec![];
        }
        buf[cursor..].to_vec()
    }

    /// Number of events in the ring buffer (for initial TUI render).
    pub async fn event_count(&self) -> usize {
        self.ring_buffer.read().await.len()
    }
}

#[cfg(test)]
mod atomic_tests {
    use super::*;

    #[tokio::test]
    async fn raw_append_builds_and_returns_length() {
        let s = SharedStore::new_in_memory();
        let key = "ns:t:tickets";
        assert_eq!(
            s.raw_append(key, serde_json::json!({"id": 1}))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            s.raw_append(key, serde_json::json!({"id": 2}))
                .await
                .unwrap(),
            2
        );
        let arr = s.raw_get(key).await.unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn raw_append_rejects_non_array_value() {
        let s = SharedStore::new_in_memory();
        let key = "ns:t:not_array";
        s.raw_set(key, serde_json::json!("oops")).await;
        let err = s.raw_append(key, serde_json::json!({"id": 1})).await;
        assert!(
            err.is_err(),
            "appending to a non-array must fail, not overwrite"
        );
    }

    #[tokio::test]
    async fn raw_incr_is_monotonic() {
        let s = SharedStore::new_in_memory();
        let key = "ns:t:counter";
        assert_eq!(s.raw_incr(key).await.unwrap(), 1);
        assert_eq!(s.raw_incr(key).await.unwrap(), 2);
        assert_eq!(s.raw_incr(key).await.unwrap(), 3);
    }

    #[tokio::test]
    async fn raw_set_if_absent_only_sets_once() {
        let s = SharedStore::new_in_memory();
        let key = "ns:t:control:mode";
        assert!(s
            .raw_set_if_absent(key, serde_json::json!("auto"))
            .await
            .unwrap());
        // A concurrent pause sets it; our second init attempt must not clobber it.
        s.raw_set(key, serde_json::json!("paused")).await;
        assert!(!s
            .raw_set_if_absent(key, serde_json::json!("auto"))
            .await
            .unwrap());
        assert_eq!(s.raw_get(key).await.unwrap(), serde_json::json!("paused"));
    }
}
