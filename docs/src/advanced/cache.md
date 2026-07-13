# Service-Level Cache

Caelix cache support is explicit service-level caching. It does not add automatic HTTP response caching.

Import `CacheModule` and inject `Cache` into a service:

```rust
use std::{sync::Arc, time::Duration};

use caelix::{Cache, CacheModule, Module, ModuleMetadata, Result, injectable};

pub struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<CacheModule>()
            .import::<UsersModule>()
    }
}

#[injectable]
pub struct UsersService {
    cache: Arc<Cache>,
}

impl UsersService {
    pub async fn find_cached(&self, id: i64) -> Result<Option<UserDto>> {
        let key = format!("users:{id}");
        if let Some(user) = self.cache.get(&key).await? {
            return Ok(Some(user));
        }

        let user = self.find(id).await?;
        if let Some(user) = &user {
            self.cache.set_with_ttl(&key, user, Duration::from_secs(60)).await?;
        }

        Ok(user)
    }
}
```

Available methods:

```rust
cache.get::<UserDto>("users:1").await?;
cache.set("users:1", &user).await?;
cache.set_with_ttl("users:1", &user, Duration::from_secs(60)).await?;
cache.set_with_optional_ttl("users:1", &user, Some(Duration::from_secs(60))).await?;
cache.delete("users:1").await?;
cache.clear().await?;
```

Values are serialized to `serde_json::Value` before storage and deserialized on `get`.

`CacheModule` registers `MemoryCacheStore` and `Cache`. `MemoryCacheStore` supports maximum entries, maximum serialized value size, and an optional default TTL through `MemoryCacheOptions`.

```rust
use std::time::Duration;

use caelix::{Cache, MemoryCacheOptions, MemoryCacheStore, Module, ModuleMetadata, provider_dependencies};

pub struct CacheConfigModule;

impl Module for CacheConfigModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<MemoryCacheStore, _, _>(provider_dependencies![], |_container| async {
                Ok::<_, std::convert::Infallible>(
                    MemoryCacheStore::with_options(MemoryCacheOptions {
                        max_entries: 10_000,
                        max_value_bytes: 2 * 1024 * 1024,
                        default_ttl: Some(Duration::from_secs(300)),
                    }),
                )
            })
            .provider::<Cache>()
    }
}
```

When the cache reaches `max_entries`, the memory store removes the oldest inserted entries. Expired entries are removed during cache writes and missed on reads.

To use a custom backend, implement `CacheStore` and register `Cache` with a factory that constructs `Cache::new(Arc<dyn CacheStore>)`.

```rust
use serde_json::Value;
use std::{sync::Arc, time::Duration};

use caelix::{BoxFuture, Cache, CacheStore, Module, ModuleMetadata, Result};

pub struct RedisCacheStore;

impl CacheStore for RedisCacheStore {
    fn get(&self, key: String) -> BoxFuture<'_, Result<Option<Value>>> {
        Box::pin(async move {
            // load JSON value by key
            Ok(None)
        })
    }

    fn set(&self, key: String, value: Value, ttl: Option<Duration>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            // write JSON value with optional TTL
            Ok(())
        })
    }

    fn delete(&self, key: String) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { Ok(()) })
    }

    fn clear(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
}

pub struct RedisCacheModule;

impl Module for RedisCacheModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider_async_factory::<Cache, _, _>(provider_dependencies![], |_container| async move {
                let store: Arc<dyn CacheStore> = Arc::new(RedisCacheStore);
                Ok::<_, std::convert::Infallible>(Cache::new(store))
            })
    }
}
```
