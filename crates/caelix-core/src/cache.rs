use crate::{
    BoxFuture, Container, Injectable, InternalServerErrorException, Module, ModuleMetadata,
    PayloadTooLargeException,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

const DEFAULT_MAX_ENTRIES: usize = 1024;
const DEFAULT_MAX_VALUE_BYTES: usize = 1024 * 1024;

pub trait CacheStore: Send + Sync + 'static {
    fn get(&self, key: String) -> BoxFuture<'_, crate::Result<Option<Value>>>;
    fn set(
        &self,
        key: String,
        value: Value,
        ttl: Option<Duration>,
    ) -> BoxFuture<'_, crate::Result<()>>;
    fn delete(&self, key: String) -> BoxFuture<'_, crate::Result<()>>;
    fn clear(&self) -> BoxFuture<'_, crate::Result<()>>;
}

#[derive(Clone)]
struct CacheEntry {
    value: Value,
    inserted_at: Instant,
    expires_at: Option<Instant>,
}

impl CacheEntry {
    fn new(value: Value, ttl: Option<Duration>) -> Self {
        let now = Instant::now();
        Self {
            value,
            inserted_at: now,
            expires_at: ttl.and_then(|ttl| now.checked_add(ttl)),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }
}

#[derive(Clone, Debug)]
pub struct MemoryCacheOptions {
    pub max_entries: usize,
    pub max_value_bytes: usize,
    pub default_ttl: Option<Duration>,
}

impl Default for MemoryCacheOptions {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            max_value_bytes: DEFAULT_MAX_VALUE_BYTES,
            default_ttl: None,
        }
    }
}

pub struct MemoryCacheStore {
    options: MemoryCacheOptions,
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl MemoryCacheStore {
    pub fn new() -> Self {
        Self::with_options(MemoryCacheOptions::default())
    }

    pub fn with_options(options: MemoryCacheOptions) -> Self {
        Self {
            options,
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.read().map_or(0, |entries| entries.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn remove_expired(entries: &mut HashMap<String, CacheEntry>) {
        entries.retain(|_, entry| !entry.is_expired());
    }

    fn evict_to_capacity(&self, entries: &mut HashMap<String, CacheEntry>) {
        if self.options.max_entries == 0 {
            entries.clear();
            return;
        }

        while entries.len() > self.options.max_entries {
            let Some(oldest_key) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.inserted_at)
                .map(|(key, _)| key.clone())
            else {
                return;
            };

            entries.remove(&oldest_key);
        }
    }
}

impl Default for MemoryCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Injectable for MemoryCacheStore {
    fn dependencies() -> Vec<crate::ProviderDependency> {
        crate::provider_dependencies![]
    }

    fn create(_container: &Container) -> BoxFuture<'_, crate::Result<Self>> {
        Box::pin(async { Ok(Self::new()) })
    }
}

impl CacheStore for MemoryCacheStore {
    fn get(&self, key: String) -> BoxFuture<'_, crate::Result<Option<Value>>> {
        Box::pin(async move {
            let mut entries = self.entries.write().map_err(|_| {
                InternalServerErrorException::new(anyhow::anyhow!("cache store lock poisoned"))
            })?;
            let Some(entry) = entries.get(&key) else {
                return Ok(None);
            };

            if entry.is_expired() {
                entries.remove(&key);
                return Ok(None);
            }

            Ok(Some(entry.value.clone()))
        })
    }

    fn set(
        &self,
        key: String,
        value: Value,
        ttl: Option<Duration>,
    ) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            let value_size = serde_json::to_vec(&value)
                .map_err(InternalServerErrorException::new)?
                .len();

            if value_size > self.options.max_value_bytes {
                return Err(PayloadTooLargeException::new(format!(
                    "cache value exceeds the configured limit of {} bytes",
                    self.options.max_value_bytes
                )));
            }

            let ttl = ttl.or(self.options.default_ttl);
            let mut entries = self.entries.write().map_err(|_| {
                InternalServerErrorException::new(anyhow::anyhow!("cache store lock poisoned"))
            })?;
            Self::remove_expired(&mut entries);
            entries.insert(key, CacheEntry::new(value, ttl));
            self.evict_to_capacity(&mut entries);
            Ok(())
        })
    }

    fn delete(&self, key: String) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            self.entries
                .write()
                .map_err(|_| {
                    InternalServerErrorException::new(anyhow::anyhow!("cache store lock poisoned"))
                })?
                .remove(&key);
            Ok(())
        })
    }

    fn clear(&self) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            self.entries
                .write()
                .map_err(|_| {
                    InternalServerErrorException::new(anyhow::anyhow!("cache store lock poisoned"))
                })?
                .clear();
            Ok(())
        })
    }
}

pub struct Cache {
    store: Arc<dyn CacheStore>,
}

impl Cache {
    pub fn new(store: Arc<dyn CacheStore>) -> Self {
        Self { store }
    }

    pub async fn get<T>(&self, key: impl Into<String>) -> crate::Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.store.get(key.into()).await? else {
            return Ok(None);
        };

        serde_json::from_value(value)
            .map(Some)
            .map_err(InternalServerErrorException::new)
    }

    pub async fn set<T>(&self, key: impl Into<String>, value: T) -> crate::Result<()>
    where
        T: Serialize,
    {
        self.set_with_optional_ttl(key, value, None).await
    }

    pub async fn set_with_ttl<T>(
        &self,
        key: impl Into<String>,
        value: T,
        ttl: Duration,
    ) -> crate::Result<()>
    where
        T: Serialize,
    {
        self.set_with_optional_ttl(key, value, Some(ttl)).await
    }

    pub async fn set_with_optional_ttl<T>(
        &self,
        key: impl Into<String>,
        value: T,
        ttl: Option<Duration>,
    ) -> crate::Result<()>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value).map_err(InternalServerErrorException::new)?;
        self.store.set(key.into(), value, ttl).await
    }

    pub async fn delete(&self, key: impl Into<String>) -> crate::Result<()> {
        self.store.delete(key.into()).await
    }

    pub async fn clear(&self) -> crate::Result<()> {
        self.store.clear().await
    }
}

impl Injectable for Cache {
    fn dependencies() -> Vec<crate::ProviderDependency> {
        crate::provider_dependencies![MemoryCacheStore]
    }

    fn create(container: &Container) -> BoxFuture<'_, crate::Result<Self>> {
        Box::pin(async move {
            let store = container.resolve::<MemoryCacheStore>()? as Arc<dyn CacheStore>;
            Ok(Self::new(store))
        })
    }
}

pub struct CacheModule;

impl Module for CacheModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<MemoryCacheStore>()
            .provider::<Cache>()
            .export::<MemoryCacheStore>()
            .export::<Cache>()
    }
}
