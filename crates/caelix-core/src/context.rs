use crate::Result;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Public Caelix type `RequestContext`.
pub struct RequestContext {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    cookies: HashMap<String, String>,
    extensions: RwLock<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl RequestContext {
    /// Runs the `new` public API operation.
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        let headers = headers
            .into_iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value))
            .collect::<HashMap<_, _>>();
        let cookies = parse_cookies(headers.get("cookie").map(String::as_str));
        Self {
            method: method.into(),
            path: path.into(),
            headers,
            cookies,
            extensions: RwLock::new(HashMap::new()),
        }
    }

    /// Runs the `method` public API operation.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Runs the `path` public API operation.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Runs the `header` public API operation.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// Runs the `bearer_token` public API operation.
    pub fn bearer_token(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ")
    }

    /// Returns the first cookie with `name`.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies.get(name).map(String::as_str)
    }

    /// Runs the `set` public API operation.
    pub fn set<T: Send + Sync + 'static>(&self, value: T) -> Result<()> {
        self.extensions
            .write()
            .map_err(|_| {
                crate::exception::startup_error("request context extensions lock poisoned")
            })?
            .insert(TypeId::of::<T>(), Arc::new(value));
        Ok(())
    }

    /// Runs the `get` public API operation.
    pub fn get<T: Send + Sync + 'static>(&self) -> Result<Option<Arc<T>>> {
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

fn parse_cookies(header: Option<&str>) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for pair in header.into_iter().flat_map(|value| value.split(';')) {
        let pair = pair.trim();
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let Ok(name) = cookie::Cookie::parse_encoded(format!("{name}=")) else {
            continue;
        };
        let Ok(value) = cookie::Cookie::parse_encoded(format!("value={}", value.trim())) else {
            continue;
        };
        cookies
            .entry(name.name().to_string())
            .or_insert_with(|| value.value().to_string());
    }
    cookies
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cookie_edge_cases_once() {
        let ctx = RequestContext::new(
            "GET",
            "/",
            HashMap::from([(
                "Cookie".into(),
                " theme = dark%20mode ; token=a=b=c; broken; =bad; dup=first; dup=second; na%6De=value"
                    .into(),
            )]),
        );
        assert_eq!(ctx.cookie("theme"), Some("dark mode"));
        assert_eq!(ctx.cookie("token"), Some("a=b=c"));
        assert_eq!(ctx.cookie("dup"), Some("first"));
        assert_eq!(ctx.cookie("name"), Some("value"));
        assert_eq!(ctx.cookie("broken"), None);
    }
}
