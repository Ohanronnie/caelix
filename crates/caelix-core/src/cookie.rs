use std::time::{Duration, SystemTime};

/// The `SameSite` attribute applied to a response cookie.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SameSite {
    /// Send only in same-site contexts.
    Strict,
    /// Send in same-site contexts and top-level safe navigations.
    Lax,
    /// Allow cross-site sending; requires `Secure` in modern browsers.
    None,
}

/// A response cookie with secure defaults.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cookie {
    name: String,
    value: String,
    http_only: bool,
    secure: bool,
    same_site: SameSite,
    path: Option<String>,
    domain: Option<String>,
    max_age: Option<Duration>,
    expires: Option<SystemTime>,
}

impl Cookie {
    /// Creates a cookie with secure framework defaults.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            http_only: true,
            secure: true,
            same_site: SameSite::Lax,
            path: Some("/".into()),
            domain: None,
            max_age: None,
            expires: None,
        }
    }

    /// Creates an expired cookie suitable for removing a stored cookie.
    pub fn removal(name: impl Into<String>) -> Self {
        Self::new(name, "")
            .max_age(Duration::ZERO)
            .expires(SystemTime::UNIX_EPOCH)
    }

    /// Sets the `HttpOnly` attribute.
    pub fn http_only(mut self, value: bool) -> Self {
        self.http_only = value;
        self
    }
    /// Sets the `Secure` attribute.
    pub fn secure(mut self, value: bool) -> Self {
        self.secure = value;
        self
    }
    /// Sets the `SameSite` attribute.
    pub fn same_site(mut self, value: SameSite) -> Self {
        self.same_site = value;
        self
    }
    /// Sets the cookie path.
    pub fn path(mut self, value: impl Into<String>) -> Self {
        self.path = Some(value.into());
        self
    }
    /// Sets the cookie domain.
    pub fn domain(mut self, value: impl Into<String>) -> Self {
        self.domain = Some(value.into());
        self
    }
    /// Sets the relative lifetime.
    pub fn max_age(mut self, value: Duration) -> Self {
        self.max_age = Some(value);
        self
    }
    /// Sets the absolute expiry time.
    pub fn expires(mut self, value: SystemTime) -> Self {
        self.expires = Some(value);
        self
    }

    /// Returns the cookie name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Returns the cookie value.
    pub fn value(&self) -> &str {
        &self.value
    }
    /// Returns whether `HttpOnly` is enabled.
    pub fn is_http_only(&self) -> bool {
        self.http_only
    }
    /// Returns whether `Secure` is enabled.
    pub fn is_secure(&self) -> bool {
        self.secure
    }
    /// Returns the `SameSite` policy.
    pub fn same_site_value(&self) -> SameSite {
        self.same_site
    }
    /// Returns the configured path.
    pub fn path_value(&self) -> Option<&str> {
        self.path.as_deref()
    }
    /// Returns the configured domain.
    pub fn domain_value(&self) -> Option<&str> {
        self.domain.as_deref()
    }
    /// Returns the configured relative lifetime.
    pub fn max_age_value(&self) -> Option<Duration> {
        self.max_age
    }
    /// Returns the configured absolute expiry time.
    pub fn expires_value(&self) -> Option<SystemTime> {
        self.expires
    }

    /// Serializes this cookie for a `Set-Cookie` header.
    pub fn to_header_value(&self) -> String {
        let mut cookie = cookie::Cookie::new(self.name.clone(), self.value.clone());
        cookie.set_http_only(self.http_only);
        cookie.set_secure(self.secure);
        cookie.set_same_site(match self.same_site {
            SameSite::Strict => cookie::SameSite::Strict,
            SameSite::Lax => cookie::SameSite::Lax,
            SameSite::None => cookie::SameSite::None,
        });
        if let Some(path) = &self.path {
            cookie.set_path(path.clone());
        }
        if let Some(domain) = &self.domain {
            cookie.set_domain(domain.clone());
        }
        if let Some(max_age) = self.max_age {
            cookie.set_max_age(
                cookie::time::Duration::try_from(max_age).unwrap_or(cookie::time::Duration::MAX),
            );
        }
        if let Some(expires) = self.expires {
            cookie.set_expires(cookie::time::OffsetDateTime::from(expires));
        }
        cookie.encoded().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_removal_are_safe() {
        let cookie = Cookie::new("session", "secret");
        assert!(cookie.is_http_only());
        assert!(cookie.is_secure());
        assert_eq!(cookie.same_site_value(), SameSite::Lax);
        assert_eq!(cookie.path_value(), Some("/"));

        let removal = Cookie::removal("session")
            .domain("example.com")
            .path("/auth");
        assert_eq!(removal.value(), "");
        assert_eq!(removal.max_age_value(), Some(Duration::ZERO));
        assert_eq!(removal.expires_value(), Some(SystemTime::UNIX_EPOCH));
        assert_eq!(removal.domain_value(), Some("example.com"));
        assert_eq!(removal.path_value(), Some("/auth"));
    }
}
