use crate::{
    BoxFuture, Container, HttpResponse, Injectable, InternalServerErrorException, Module,
    ModuleMetadata, ProviderDependency, RequestContext, Result, TooManyRequestsException,
};
use ipnet::IpNet;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, VecDeque},
    marker::PhantomData,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// A fixed-window request policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThrottlePolicy {
    /// Maximum attempts accepted during the window.
    pub limit: u64,
    /// Window duration.
    pub window: Duration,
}

impl ThrottlePolicy {
    /// Creates a policy from a positive request limit and window length.
    pub const fn new(limit: u64, window_seconds: u64) -> Self {
        Self {
            limit,
            window: Duration::from_secs(window_seconds),
        }
    }

    /// Validates that this policy can be represented by the runtime clock.
    pub fn validate(self) -> Result<()> {
        validate_policy(self)
    }
}

/// The result of an atomic store increment.
#[derive(Clone, Copy, Debug)]
pub struct ThrottleStoreRecord {
    /// Count after the increment.
    pub count: u64,
    /// Time remaining in the current window.
    pub retry_after: Duration,
}

/// Atomic storage used by the throttler.
pub trait ThrottleStore: Send + Sync + 'static {
    /// Atomically increments `key` in its first-hit fixed window.
    fn increment<'a>(
        &'a self,
        key: &'a str,
        window: Duration,
    ) -> BoxFuture<'a, Result<ThrottleStoreRecord>>;
}

struct MemoryBucket {
    count: u64,
    expires_at: Instant,
    generation: u64,
}

#[derive(Default)]
struct MemoryThrottleState {
    buckets: HashMap<String, MemoryBucket>,
    expirations: BinaryHeap<Reverse<(Instant, u64, String)>>,
    insertion_order: VecDeque<(u64, String)>,
    next_generation: u64,
}

/// A bounded, process-local atomic throttle store.
pub struct MemoryThrottleStore {
    state: Mutex<MemoryThrottleState>,
    capacity: usize,
}

impl MemoryThrottleStore {
    /// Creates a store with the default capacity of 100,000 active buckets.
    pub fn new() -> Self {
        Self::with_capacity(100_000)
    }

    /// Creates a store with a configurable active-bucket capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            state: Mutex::new(MemoryThrottleState::default()),
            capacity: capacity.max(1),
        }
    }
}

impl Default for MemoryThrottleStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ThrottleStore for MemoryThrottleStore {
    fn increment<'a>(
        &'a self,
        key: &'a str,
        window: Duration,
    ) -> BoxFuture<'a, Result<ThrottleStoreRecord>> {
        Box::pin(async move {
            let now = Instant::now();
            let mut state = self.state.lock().map_err(|_| {
                InternalServerErrorException::new(std::io::Error::other(
                    "throttle store lock poisoned",
                ))
            })?;
            while let Some(Reverse((expires_at, generation, expired_key))) =
                state.expirations.peek().cloned()
            {
                if expires_at > now {
                    break;
                }
                state.expirations.pop();
                if state.buckets.get(&expired_key).is_some_and(|bucket| {
                    bucket.generation == generation && bucket.expires_at <= now
                }) {
                    state.buckets.remove(&expired_key);
                }
            }
            let new_expires_at = if state.buckets.contains_key(key) {
                None
            } else {
                Some(now.checked_add(window).ok_or_else(|| {
                    crate::exception::startup_error("throttle window exceeds the clock range")
                })?)
            };
            if !state.buckets.contains_key(key) && state.buckets.len() >= self.capacity {
                while let Some((generation, oldest)) = state.insertion_order.pop_front() {
                    if state
                        .buckets
                        .get(&oldest)
                        .is_some_and(|bucket| bucket.generation == generation)
                    {
                        state.buckets.remove(&oldest);
                        break;
                    }
                }
            }
            if !state.buckets.contains_key(key) {
                let owned_key = key.to_owned();
                let expires_at =
                    new_expires_at.expect("new throttle bucket must have an expiration");
                state.next_generation = state.next_generation.wrapping_add(1);
                let generation = state.next_generation;
                state.buckets.insert(
                    owned_key.clone(),
                    MemoryBucket {
                        count: 0,
                        expires_at,
                        generation,
                    },
                );
                state
                    .expirations
                    .push(Reverse((expires_at, generation, owned_key.clone())));
                state.insertion_order.push_back((generation, owned_key));
            }
            if state.expirations.len() > self.capacity.saturating_mul(2) {
                state.expirations = state
                    .buckets
                    .iter()
                    .map(|(key, bucket)| {
                        Reverse((bucket.expires_at, bucket.generation, key.clone()))
                    })
                    .collect();
            }
            if state.insertion_order.len() > self.capacity.saturating_mul(2) {
                let mut active = state
                    .buckets
                    .iter()
                    .map(|(key, bucket)| (bucket.generation, key.clone()))
                    .collect::<Vec<_>>();
                active.sort_unstable_by_key(|(generation, _)| *generation);
                state.insertion_order = active.into();
            }
            let bucket = state
                .buckets
                .get_mut(key)
                .expect("throttle bucket was just inserted");
            bucket.count = bucket.count.saturating_add(1);
            Ok(ThrottleStoreRecord {
                count: bucket.count,
                retry_after: bucket.expires_at.saturating_duration_since(now),
            })
        })
    }
}

/// Derives the client identity used in throttle keys.
pub trait ThrottleTracker: Send + Sync + 'static {
    /// Returns a stable identity for this request.
    fn track<'a>(&'a self, context: &'a RequestContext) -> BoxFuture<'a, Result<String>>;
}

/// Tracks clients by IP, with explicit trusted-proxy support.
pub struct IpThrottleTracker {
    trusted_proxies: Vec<IpNet>,
}

impl IpThrottleTracker {
    /// Creates an IP tracker that trusts no forwarding headers.
    pub fn new() -> Self {
        Self {
            trusted_proxies: Vec::new(),
        }
    }

    /// Creates a tracker with trusted proxy IP addresses or CIDR ranges.
    pub fn with_trusted_proxies<I, S>(proxies: I) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let trusted_proxies = proxies
            .into_iter()
            .map(|value| {
                value.as_ref().parse::<IpNet>().map_err(|error| {
                    crate::exception::startup_error(format!(
                        "invalid trusted proxy '{}': {error}",
                        value.as_ref()
                    ))
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { trusted_proxies })
    }

    fn trusted(&self, address: IpAddr) -> bool {
        self.trusted_proxies
            .iter()
            .any(|network| network.contains(&address))
    }
}

impl Default for IpThrottleTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ThrottleTracker for IpThrottleTracker {
    fn track<'a>(&'a self, context: &'a RequestContext) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            let peer = context.peer_addr().ok_or_else(|| {
                InternalServerErrorException::new(std::io::Error::other(
                    "request peer address is unavailable",
                ))
            })?;
            if !self.trusted(peer.ip()) {
                return Ok(peer.ip().to_string());
            }
            let Some(forwarded) = context.header("x-forwarded-for") else {
                return Ok(peer.ip().to_string());
            };
            let chain = forwarded
                .split(',')
                .map(|value| value.trim().parse::<IpAddr>())
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| {
                    InternalServerErrorException::new(std::io::Error::other(
                        "malformed X-Forwarded-For header",
                    ))
                })?;
            if chain.is_empty() {
                return Err(InternalServerErrorException::new(std::io::Error::other(
                    "malformed X-Forwarded-For header",
                )));
            }
            Ok(chain
                .iter()
                .rev()
                .find(|address| !self.trusted(**address))
                .copied()
                .unwrap_or(peer.ip())
                .to_string())
        })
    }
}

/// Runtime throttling configuration.
#[derive(Clone)]
pub struct ThrottleOptions {
    /// Global policy applied to unannotated controller routes.
    pub policy: ThrottlePolicy,
    /// Whether rejected responses include `Retry-After`.
    pub retry_after_header: bool,
    /// Atomic counter storage.
    pub store: Arc<dyn ThrottleStore>,
    /// Client identity tracker.
    pub tracker: Arc<dyn ThrottleTracker>,
}

impl Default for ThrottleOptions {
    fn default() -> Self {
        Self {
            policy: ThrottlePolicy::new(60, 60),
            retry_after_header: true,
            store: Arc::new(MemoryThrottleStore::new()),
            tracker: Arc::new(IpThrottleTracker::new()),
        }
    }
}

impl ThrottleOptions {
    /// Replaces the atomic store.
    pub fn with_store(mut self, store: Arc<dyn ThrottleStore>) -> Self {
        self.store = store;
        self
    }

    /// Replaces the client tracker.
    pub fn with_tracker(mut self, tracker: Arc<dyn ThrottleTracker>) -> Self {
        self.tracker = tracker;
        self
    }

    /// Controls whether rejected responses carry `Retry-After`.
    pub fn with_retry_after_header(mut self, enabled: bool) -> Self {
        self.retry_after_header = enabled;
        self
    }
}

/// Supplies options for [`ThrottleModule`].
pub trait ThrottleConfig: Send + Sync + 'static {
    /// Modules whose exported providers are used while building options.
    fn imports() -> Vec<crate::ModuleDef> {
        vec![]
    }

    /// Providers that may be resolved while building throttle options.
    fn dependencies() -> Vec<ProviderDependency> {
        vec![]
    }

    /// Builds the application throttle options.
    fn options(container: &Container) -> Result<ThrottleOptions>;
}

/// Default 60 requests per 60 seconds throttle configuration.
pub struct DefaultThrottleConfig;

impl ThrottleConfig for DefaultThrottleConfig {
    fn options(_: &Container) -> Result<ThrottleOptions> {
        Ok(ThrottleOptions::default())
    }
}

/// The global request-throttling service.
pub struct Throttle {
    options: ThrottleOptions,
}

impl Throttle {
    /// Creates a throttle service from application options.
    pub fn new(options: ThrottleOptions) -> Result<Self> {
        validate_policy(options.policy)?;
        Ok(Self { options })
    }

    /// Checks and increments the quota, returning a response only when rejected.
    pub async fn check(
        &self,
        context: &RequestContext,
        method: &str,
        route: &str,
        policy: ThrottlePolicy,
    ) -> Result<Option<HttpResponse>> {
        validate_policy(policy)?;
        let client = self.options.tracker.track(context).await?;
        let key = format!(
            "{}:{client}{}:{method}{}:{route}",
            client.len(),
            method.len(),
            route.len()
        );
        let record = self.options.store.increment(&key, policy.window).await?;
        if record.count <= policy.limit {
            return Ok(None);
        }
        let mut response = crate::IntoCaelixResponse::into_response(TooManyRequestsException::new(
            "Rate limit exceeded",
        ));
        if self.options.retry_after_header {
            let seconds = (record.retry_after.as_secs()
                + u64::from(record.retry_after.subsec_nanos() > 0))
            .max(1);
            response.insert_header("Retry-After", seconds.to_string());
        }
        Ok(Some(response))
    }

    /// Returns the configured global policy.
    pub fn policy(&self) -> ThrottlePolicy {
        self.options.policy
    }
}

/// Global module enabling throttling for macro-generated controller routes.
pub struct ThrottleModule<C = DefaultThrottleConfig>(PhantomData<C>);

impl<C: ThrottleConfig> Module for ThrottleModule<C> {
    fn register() -> ModuleMetadata {
        let mut metadata = ModuleMetadata::global();
        metadata.imports.extend(C::imports());
        metadata
            .provider_async_factory::<Throttle, _, crate::HttpException>(
                C::dependencies(),
                |container| async move {
                    let options = C::options(&container)?;
                    Throttle::new(options)
                },
            )
            .export::<Throttle>()
    }
}

fn validate_policy(policy: ThrottlePolicy) -> Result<()> {
    if policy.limit == 0 || policy.window.is_zero() {
        return Err(crate::exception::startup_error(
            "throttle limit and window must be greater than zero",
        ));
    }
    Instant::now().checked_add(policy.window).ok_or_else(|| {
        crate::exception::startup_error("throttle window exceeds the clock range")
    })?;
    Ok(())
}

impl Injectable for Throttle {
    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Throttle::new(ThrottleOptions::default()) })
    }

    fn dependencies() -> Vec<ProviderDependency> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    };

    #[tokio::test]
    async fn increments_atomically_and_isolates_keys() {
        let store = Arc::new(MemoryThrottleStore::new());
        let mut tasks = Vec::new();
        for _ in 0..100 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                store
                    .increment("a", Duration::from_secs(60))
                    .await
                    .unwrap()
                    .count
            }));
        }
        let mut counts = Vec::new();
        for task in tasks {
            counts.push(task.await.unwrap());
        }
        counts.sort_unstable();
        assert_eq!(counts, (1..=100).collect::<Vec<_>>());
        assert_eq!(
            store
                .increment("b", Duration::from_secs(60))
                .await
                .unwrap()
                .count,
            1
        );
    }

    #[tokio::test]
    async fn evicts_oldest_bucket_at_capacity() {
        let store = MemoryThrottleStore::with_capacity(1);
        store
            .increment("old", Duration::from_secs(60))
            .await
            .unwrap();
        store
            .increment("new", Duration::from_secs(60))
            .await
            .unwrap();
        assert_eq!(
            store
                .increment("old", Duration::from_secs(60))
                .await
                .unwrap()
                .count,
            1
        );
    }

    #[tokio::test]
    async fn cleanup_uses_each_buckets_own_window() {
        let store = MemoryThrottleStore::new();
        store
            .increment("long", Duration::from_secs(1))
            .await
            .unwrap();
        store
            .increment("short", Duration::from_millis(10))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        store
            .increment("cleanup", Duration::from_millis(10))
            .await
            .unwrap();
        assert_eq!(
            store
                .increment("long", Duration::from_secs(1))
                .await
                .unwrap()
                .count,
            2
        );
    }

    #[tokio::test]
    async fn stale_generations_do_not_change_oldest_active_eviction() {
        let store = MemoryThrottleStore::with_capacity(2);
        store
            .increment("a", Duration::from_millis(10))
            .await
            .unwrap();
        store.increment("b", Duration::from_secs(60)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        store.increment("c", Duration::from_secs(60)).await.unwrap();
        store.increment("a", Duration::from_secs(60)).await.unwrap();
        assert_eq!(
            store
                .increment("b", Duration::from_secs(60))
                .await
                .unwrap()
                .count,
            1
        );
        assert_eq!(
            store
                .increment("a", Duration::from_secs(60))
                .await
                .unwrap()
                .count,
            2
        );
    }

    #[tokio::test]
    async fn auxiliary_indexes_remain_bounded_during_churn() {
        let store = MemoryThrottleStore::with_capacity(4);
        for index in 0..100 {
            store
                .increment(&format!("key-{index}"), Duration::from_secs(60))
                .await
                .unwrap();
        }
        let state = store.state.lock().unwrap();
        assert!(state.buckets.len() <= 4);
        assert!(state.expirations.len() <= 8);
        assert!(state.insertion_order.len() <= 8);
    }

    #[tokio::test]
    async fn failed_new_bucket_does_not_evict_an_active_bucket() {
        let store = MemoryThrottleStore::with_capacity(1);
        store
            .increment("active", Duration::from_secs(60))
            .await
            .unwrap();
        assert!(
            store
                .increment("overflow", Duration::from_secs(u64::MAX))
                .await
                .is_err()
        );
        assert_eq!(
            store
                .increment("active", Duration::from_secs(60))
                .await
                .unwrap()
                .count,
            2
        );
    }

    #[test]
    fn rejects_zero_programmatic_policies() {
        let mut options = ThrottleOptions::default();
        options.policy = ThrottlePolicy::new(0, 60);
        assert!(Throttle::new(options).is_err());

        let mut options = ThrottleOptions::default();
        options.policy = ThrottlePolicy::new(1, 0);
        assert!(Throttle::new(options).is_err());

        let mut options = ThrottleOptions::default();
        options.policy = ThrottlePolicy::new(1, u64::MAX);
        assert!(Throttle::new(options).is_err());
    }

    #[tokio::test]
    async fn ip_tracker_ignores_forwarding_from_untrusted_peers() {
        let tracker = IpThrottleTracker::new();
        let context = RequestContext::new(
            "GET",
            "/",
            HashMap::from([("X-Forwarded-For".into(), "203.0.113.9".into())]),
        )
        .with_peer_addr(SocketAddr::new(Ipv4Addr::new(192, 0, 2, 4).into(), 80));
        assert_eq!(
            tracker.track(&context).await.unwrap(),
            Ipv4Addr::new(192, 0, 2, 4).to_string()
        );
    }

    #[tokio::test]
    async fn ip_tracker_walks_trusted_proxy_chain_right_to_left() {
        let tracker =
            IpThrottleTracker::with_trusted_proxies(["10.0.0.0/8", "192.0.2.0/24"]).unwrap();
        let context = RequestContext::new(
            "GET",
            "/",
            HashMap::from([(
                "X-Forwarded-For".into(),
                "2001:db8::7, 192.0.2.8, 10.1.1.3".into(),
            )]),
        )
        .with_peer_addr(SocketAddr::new(Ipv4Addr::new(10, 0, 0, 2).into(), 80));
        assert_eq!(tracker.track(&context).await.unwrap(), "2001:db8::7");
    }

    #[tokio::test]
    async fn ip_tracker_fails_closed_for_malformed_trusted_chain_and_missing_peer() {
        let tracker = IpThrottleTracker::with_trusted_proxies(["::1/128"]).unwrap();
        let malformed = RequestContext::new(
            "GET",
            "/",
            HashMap::from([("X-Forwarded-For".into(), "not-an-ip".into())]),
        )
        .with_peer_addr(SocketAddr::new(Ipv6Addr::LOCALHOST.into(), 80));
        assert!(tracker.track(&malformed).await.is_err());
        assert!(
            tracker
                .track(&RequestContext::new("GET", "/", HashMap::new()))
                .await
                .is_err()
        );
    }
}
