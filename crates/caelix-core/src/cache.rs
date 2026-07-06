use crate::{
    BoxFuture, Container, Injectable, InternalServerErrorException, Module, ModuleMetadata,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

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
    expires_at: Option<Instant>,
}

impl CacheEntry {
    fn new(value: Value, ttl: Option<Duration>) -> Self {
        Self {
            value,
            expires_at: ttl.map(|ttl| Instant::now() + ttl),
        }
    }

    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }
}

pub struct MemoryCacheStore {
    entries: RwLock<HashMap<String, CacheEntry>>,
}

impl MemoryCacheStore {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.entries
            .read()
            .expect("cache store lock poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for MemoryCacheStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Injectable for MemoryCacheStore {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async { Self::new() })
    }
}

impl CacheStore for MemoryCacheStore {
    fn get(&self, key: String) -> BoxFuture<'_, crate::Result<Option<Value>>> {
        Box::pin(async move {
            let mut entries = self.entries.write().expect("cache store lock poisoned");
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
            self.entries
                .write()
                .expect("cache store lock poisoned")
                .insert(key, CacheEntry::new(value, ttl));
            Ok(())
        })
    }

    fn delete(&self, key: String) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            self.entries
                .write()
                .expect("cache store lock poisoned")
                .remove(&key);
            Ok(())
        })
    }

    fn clear(&self) -> BoxFuture<'_, crate::Result<()>> {
        Box::pin(async move {
            self.entries
                .write()
                .expect("cache store lock poisoned")
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
    fn create(container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async move {
            let store = container.resolve::<MemoryCacheStore>() as Arc<dyn CacheStore>;
            Self::new(store)
        })
    }
}

pub struct CacheModule;

impl Module for CacheModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<MemoryCacheStore>()
            .provider::<Cache>()
    }
}
