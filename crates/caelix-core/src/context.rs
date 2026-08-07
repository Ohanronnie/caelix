use crate::Result;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, OnceLock, RwLock},
};

/// Public Caelix type `RequestContext`.
pub struct RequestContext {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    cookies: OnceLock<HashMap<String, String>>,
    peer_addr: Option<SocketAddr>,
    extensions: RwLock<Option<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>>,
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
        Self::from_normalized_headers(method, path, headers)
    }

    /// Builds a context from already lowercase header names without copying them again.
    #[doc(hidden)]
    pub fn from_normalized_headers(
        method: impl Into<String>,
        path: impl Into<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            headers,
            cookies: OnceLock::new(),
            peer_addr: None,
            extensions: RwLock::new(None),
        }
    }

    /// Sets the immediate socket peer address.
    pub fn with_peer_addr(mut self, peer_addr: SocketAddr) -> Self {
        self.peer_addr = Some(peer_addr);
        self
    }

    /// Returns the immediate socket peer address, when the runtime supplied one.
    pub fn peer_addr(&self) -> Option<SocketAddr> {
        self.peer_addr
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
            .get(name)
            .or_else(|| {
                name.bytes()
                    .any(|byte| byte.is_ascii_uppercase())
                    .then(|| self.headers.get(&name.to_ascii_lowercase()))
                    .flatten()
            })
            .map(String::as_str)
    }

    /// Runs the `bearer_token` public API operation.
    pub fn bearer_token(&self) -> Option<&str> {
        self.header("authorization")?.strip_prefix("Bearer ")
    }

    /// Returns the first cookie with `name`.
    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies
            .get_or_init(|| parse_cookies(self.headers.get("cookie").map(String::as_str)))
            .get(name)
            .map(String::as_str)
    }

    /// Runs the `set` public API operation.
    pub fn set<T: Send + Sync + 'static>(&self, value: T) -> Result<()> {
        self.extensions
            .write()
            .map_err(|_| {
                crate::exception::startup_error("request context extensions lock poisoned")
            })?
            .get_or_insert_with(HashMap::new)
            .insert(TypeId::of::<T>(), Arc::new(value));
        Ok(())
    }

    /// Runs the `get` public API operation.
    pub fn get<T: Send + Sync + 'static>(&self) -> Result<Option<Arc<T>>> {
        let extensions = self.extensions.read().map_err(|_| {
            crate::exception::startup_error("request context extensions lock poisoned")
        })?;
        let value = extensions
            .as_ref()
            .and_then(|extensions| extensions.get(&TypeId::of::<T>()))
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

    #[test]
    fn defers_cookie_parsing_and_extension_storage_until_used() {
        let ctx = RequestContext::new(
            "GET",
            "/",
            HashMap::from([("Cookie".into(), "session=abc".into())]),
        );

        assert!(ctx.cookies.get().is_none());
        assert!(ctx.extensions.read().unwrap().is_none());
        assert_eq!(ctx.get::<String>().unwrap(), None);
        assert!(ctx.extensions.read().unwrap().is_none());

        assert_eq!(ctx.cookie("session"), Some("abc"));
        assert!(ctx.cookies.get().is_some());

        ctx.set("authenticated".to_string()).unwrap();
        assert!(ctx.extensions.read().unwrap().is_some());
    }

    #[test]
    fn treats_correlation_headers_as_ordinary_headers() {
        let ctx = RequestContext::new(
            "GET",
            "/orders",
            HashMap::from([
                ("X-Request-Id".into(), "request-123".into()),
                ("X-Trace-Id".into(), "trace-456".into()),
                (
                    "traceparent".into(),
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
                ),
            ]),
        );

        assert_eq!(ctx.header("x-request-id"), Some("request-123"));
        assert_eq!(ctx.header("X-Trace-Id"), Some("trace-456"));
        assert_eq!(
            ctx.header("traceparent"),
            Some("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
        );
    }
}
