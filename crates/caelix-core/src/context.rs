use crate::Result;
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

/// Public Caelix type `RequestContext`.
pub struct RequestContext {
    method: String,
    path: String,
    request_id: String,
    trace_id: String,
    headers: HashMap<String, String>,
    cookies: HashMap<String, String>,
    peer_addr: Option<SocketAddr>,
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
        let request_id = headers
            .get("x-request-id")
            .and_then(|value| valid_correlation_id(value))
            .map(str::to_owned)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let trace_id = headers
            .get("traceparent")
            .and_then(|value| trace_id_from_traceparent(value))
            .or_else(|| {
                headers
                    .get("x-trace-id")
                    .and_then(|value| valid_correlation_id(value))
            })
            .unwrap_or(&request_id)
            .to_owned();
        Self {
            method: method.into(),
            path: path.into(),
            request_id,
            trace_id,
            headers,
            cookies,
            peer_addr: None,
            extensions: RwLock::new(HashMap::new()),
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

    /// Returns the accepted incoming `X-Request-Id`, or a generated UUID.
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    /// Returns the W3C `traceparent` trace ID, `X-Trace-Id`, or request ID fallback.
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// Adds this request's correlation identifiers to an HTTP response.
    pub fn attach_correlation_headers(
        &self,
        mut response: crate::HttpResponse,
    ) -> crate::HttpResponse {
        response.headers.retain(|(name, _)| {
            !name.eq_ignore_ascii_case("x-request-id") && !name.eq_ignore_ascii_case("x-trace-id")
        });
        response.insert_header("X-Request-Id", self.request_id.clone());
        response.insert_header("X-Trace-Id", self.trace_id.clone());
        response
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

fn valid_correlation_id(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then_some(value)
}

fn trace_id_from_traceparent(value: &str) -> Option<&str> {
    let mut parts = value.trim().split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some()
        || version.len() != 2
        || trace_id.len() != 32
        || parent_id.len() != 16
        || flags.len() != 2
        || trace_id.bytes().all(|byte| byte == b'0')
        || !version
            .bytes()
            .chain(trace_id.bytes())
            .chain(parent_id.bytes())
            .chain(flags.bytes())
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(trace_id)
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
    fn derives_request_and_trace_ids_from_headers() {
        let ctx = RequestContext::new(
            "GET",
            "/orders",
            HashMap::from([
                ("X-Request-Id".into(), "request-123".into()),
                (
                    "traceparent".into(),
                    "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".into(),
                ),
            ]),
        );

        assert_eq!(ctx.request_id(), "request-123");
        assert_eq!(ctx.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
    }

    #[test]
    fn generates_safe_correlation_ids_for_missing_or_invalid_headers() {
        let ctx = RequestContext::new(
            "POST",
            "/orders",
            HashMap::from([("X-Request-Id".into(), "unsafe\nvalue".into())]),
        );

        assert!(uuid::Uuid::parse_str(ctx.request_id()).is_ok());
        assert_eq!(ctx.trace_id(), ctx.request_id());
    }

    #[test]
    fn attaches_correlation_ids_to_responses() {
        let ctx = RequestContext::new(
            "GET",
            "/orders",
            HashMap::from([
                ("X-Request-Id".into(), "request-123".into()),
                ("X-Trace-Id".into(), "trace-456".into()),
            ]),
        );
        let response =
            ctx.attach_correlation_headers(crate::HttpResponse::text(http::StatusCode::OK, "ok"));

        assert!(
            response
                .headers
                .contains(&("X-Request-Id".into(), "request-123".into()))
        );
        assert!(
            response
                .headers
                .contains(&("X-Trace-Id".into(), "trace-456".into()))
        );
    }
}
