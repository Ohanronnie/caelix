use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub struct RequestContext {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    extensions: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl RequestContext {
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            headers: headers
                .into_iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value))
                .collect(),
            extensions: RwLock::new(HashMap::new()),
        }
    }

    pub fn method(&self) -> &str {
        &self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn bearer_token(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ")
    }

    pub fn set<T: Send + Sync + 'static>(&self, value: T) -> crate::Result<()> {
        self.extensions
            .write()
            .map_err(|_| {
                crate::exception::startup_error("request context extensions lock poisoned")
            })?
            .insert(TypeId::of::<T>(), Arc::new(value));
        Ok(())
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> crate::Result<Option<Arc<T>>> {
        let value = self
            .extensions
            .read()
            .map_err(|_| {
                crate::exception::startup_error("request context extensions lock poisoned")
            })?
            .get(&TypeId::of::<T>())
            .cloned();

        let Some(value) = value else {
            return Ok(None);
        };

        value
            .clone()
            .downcast::<T>()
            .map(Some)
            .map_err(|_| crate::exception::startup_error("request context extension type mismatch"))
    }
}
