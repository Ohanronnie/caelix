//! OpenAPI document construction shared by Caelix runtime adapters.

use crate::{HttpException, Module, Result, StatusCode};
use std::collections::BTreeMap;
/// `utoipa` OpenAPI security component types, re-exported for configuration.
pub use utoipa::openapi::security;
use utoipa::openapi::{
    Components, Content, Info, OpenApi, Ref, RefOr, Required,
    path::{HttpMethod, Operation, Parameter, ParameterIn},
    request_body::RequestBody,
    response::Response,
    schema::{AllOfBuilder, ArrayBuilder, KnownFormat, ObjectBuilder, Schema, SchemaFormat, Type},
};
/// Re-exported so applications using Caelix's `openapi` feature do not need a
/// separate `utoipa` dependency for derives such as `ToSchema`.
pub use utoipa::{self, IntoParams, PartialSchema, ToSchema};

/// A documentation-only security requirement for one controller route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Public Caelix enumeration `Security`.
pub enum Security {
    /// Public Caelix API.
    BearerAuth,
    /// Public Caelix API.
    ApiKeyAuth,
    /// Public Caelix API.
    CookieAuth,
    /// Public Caelix API.
    OAuth2(&'static [&'static str]),
    /// Public Caelix API.
    Custom {
        /// OpenAPI security-scheme name registered in [`OpenApiConfig`].
        name: &'static str,
        /// OAuth-style scopes required by the operation.
        scopes: &'static [&'static str],
    },
}

impl Security {
    fn name_and_scopes(&self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::BearerAuth => ("BearerAuth", &[]),
            Self::ApiKeyAuth => ("ApiKeyAuth", &[]),
            Self::CookieAuth => ("CookieAuth", &[]),
            Self::OAuth2(scopes) => ("OAuth2", scopes),
            Self::Custom { name, scopes } => (name, scopes),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfiguredSecurityKind {
    BearerAuth,
    ApiKeyAuth,
    CookieAuth,
    OAuth2,
    Custom,
}

#[derive(Clone, PartialEq, Eq)]
struct ConfiguredSecurityScheme {
    scheme: security::SecurityScheme,
    kind: ConfiguredSecurityKind,
}

/// Opt-in OpenAPI configuration for a runtime `Application`.
#[derive(Clone, Eq, PartialEq)]
/// Public Caelix type `OpenApiConfig`.
pub struct OpenApiConfig {
    /// The `title` value.
    pub title: String,
    /// The `version` value.
    pub version: String,
    /// The `json_path` value.
    pub json_path: String,
    /// The `ui_path` value.
    pub ui_path: String,
    security_schemes: BTreeMap<String, ConfiguredSecurityScheme>,
}

impl OpenApiConfig {
    /// Runs the `new` public API operation.
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            json_path: "/openapi.json".into(),
            ui_path: "/docs".into(),
            security_schemes: BTreeMap::new(),
        }
    }

    /// Changes the URL where the JSON document is served.
    pub fn json_path(mut self, path: impl Into<String>) -> Self {
        self.json_path = normalize_path(path.into());
        self
    }

    /// Changes the URL where Swagger UI is served.
    pub fn ui_path(mut self, path: impl Into<String>) -> Self {
        self.ui_path = normalize_path(path.into());
        self
    }

    /// Registers the standard `BearerAuth` HTTP bearer JWT scheme.
    pub fn bearer_auth(mut self) -> Self {
        self.insert_security_scheme(
            "BearerAuth",
            security::SecurityScheme::Http(
                security::HttpBuilder::new()
                    .scheme(security::HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
            ConfiguredSecurityKind::BearerAuth,
        );
        self
    }

    /// Registers the standard `ApiKeyAuth` request-header API key scheme.
    pub fn api_key_auth(mut self, header_name: impl Into<String>) -> Self {
        self.insert_security_scheme(
            "ApiKeyAuth",
            security::SecurityScheme::ApiKey(security::ApiKey::Header(security::ApiKeyValue::new(
                header_name,
            ))),
            ConfiguredSecurityKind::ApiKeyAuth,
        );
        self
    }

    /// Registers the standard `CookieAuth` cookie API key scheme.
    pub fn cookie_auth(mut self, cookie_name: impl Into<String>) -> Self {
        self.insert_security_scheme(
            "CookieAuth",
            security::SecurityScheme::ApiKey(security::ApiKey::Cookie(security::ApiKeyValue::new(
                cookie_name,
            ))),
            ConfiguredSecurityKind::CookieAuth,
        );
        self
    }

    /// Registers OAuth2 flows under the standard `OAuth2` scheme name.
    pub fn oauth2(mut self, flow_config: security::OAuth2) -> Self {
        self.insert_security_scheme(
            "OAuth2",
            security::SecurityScheme::OAuth2(flow_config),
            ConfiguredSecurityKind::OAuth2,
        );
        self
    }

    /// Registers an application-defined security scheme for `Security::Custom`.
    pub fn security_scheme(
        mut self,
        name: impl Into<String>,
        scheme: security::SecurityScheme,
    ) -> Self {
        let name = name.into();
        self.insert_security_scheme(name, scheme, ConfiguredSecurityKind::Custom);
        self
    }

    fn insert_security_scheme(
        &mut self,
        name: impl Into<String>,
        scheme: security::SecurityScheme,
        kind: ConfiguredSecurityKind,
    ) {
        self.security_schemes
            .insert(name.into(), ConfiguredSecurityScheme { scheme, kind });
    }
}

fn normalize_path(mut path: String) -> String {
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    path
}

/// Metadata attached by the controller macro to one documented route.
#[doc(hidden)]
pub struct OpenApiRouteDef {
    /// The `document` value.
    pub document: fn(&mut OpenApi),
}

/// Describes an exception marker that can appear in `#[errors(...)]`.
pub trait OpenApiError {
    /// Public Caelix API.
    fn status() -> StatusCode;
    /// Public Caelix API.
    fn description() -> &'static str {
        "Error response"
    }
}

macro_rules! exception_openapi_errors {
    ($($type:ident => $status:ident),* $(,)?) => {$(
        impl OpenApiError for crate::$type {
            fn status() -> StatusCode { StatusCode::$status }
        }
    )*};
}

exception_openapi_errors!(
    BadRequestException => BAD_REQUEST,
    UnauthorizedException => UNAUTHORIZED,
    PaymentRequiredException => PAYMENT_REQUIRED,
    ForbiddenException => FORBIDDEN,
    NotFoundException => NOT_FOUND,
    MethodNotAllowedException => METHOD_NOT_ALLOWED,
    NotAcceptableException => NOT_ACCEPTABLE,
    ProxyAuthenticationRequiredException => PROXY_AUTHENTICATION_REQUIRED,
    RequestTimeoutException => REQUEST_TIMEOUT,
    ConflictException => CONFLICT,
    GoneException => GONE,
    LengthRequiredException => LENGTH_REQUIRED,
    PreconditionFailedException => PRECONDITION_FAILED,
    PayloadTooLargeException => PAYLOAD_TOO_LARGE,
    UriTooLongException => URI_TOO_LONG,
    UnsupportedMediaTypeException => UNSUPPORTED_MEDIA_TYPE,
    RangeNotSatisfiableException => RANGE_NOT_SATISFIABLE,
    ExpectationFailedException => EXPECTATION_FAILED,
    ImATeapotException => IM_A_TEAPOT,
    MisdirectedRequestException => MISDIRECTED_REQUEST,
    UnprocessableEntityException => UNPROCESSABLE_ENTITY,
    LockedException => LOCKED,
    FailedDependencyException => FAILED_DEPENDENCY,
    TooEarlyException => TOO_EARLY,
    UpgradeRequiredException => UPGRADE_REQUIRED,
    PreconditionRequiredException => PRECONDITION_REQUIRED,
    TooManyRequestsException => TOO_MANY_REQUESTS,
    RequestHeaderFieldsTooLargeException => REQUEST_HEADER_FIELDS_TOO_LARGE,
    UnavailableForLegalReasonsException => UNAVAILABLE_FOR_LEGAL_REASONS,
    InternalServerErrorException => INTERNAL_SERVER_ERROR,
    NotImplementedException => NOT_IMPLEMENTED,
    BadGatewayException => BAD_GATEWAY,
    ServiceUnavailableException => SERVICE_UNAVAILABLE,
    GatewayTimeoutException => GATEWAY_TIMEOUT,
    HttpVersionNotSupportedException => HTTP_VERSION_NOT_SUPPORTED,
    VariantAlsoNegotiatesException => VARIANT_ALSO_NEGOTIATES,
    InsufficientStorageException => INSUFFICIENT_STORAGE,
    LoopDetectedException => LOOP_DETECTED,
);

#[derive(serde::Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    error: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    errors: Option<BTreeMap<String, Vec<String>>>,
}

/// Builds one immutable OpenAPI 3.1 document from a module and all its imports.
pub fn build_openapi<M: Module + 'static>(config: &OpenApiConfig) -> Result<OpenApi> {
    let mut openapi = OpenApi::new(
        Info::new(config.title.clone(), config.version.clone()),
        utoipa::openapi::Paths::new(),
    );
    crate::visit_module_openapi_routes::<M>(&mut |route| (route.document)(&mut openapi));
    add_security_schemes(&mut openapi, config);
    validate_security_requirements(&openapi, config)?;
    validate_document_paths(&openapi, config)?;
    Ok(openapi)
}

fn add_security_schemes(openapi: &mut OpenApi, config: &OpenApiConfig) {
    if config.security_schemes.is_empty() {
        return;
    }
    let components = openapi.components.get_or_insert_with(Components::new);
    for (name, configured) in &config.security_schemes {
        components
            .security_schemes
            .insert(name.clone(), configured.scheme.clone());
    }
}

fn validate_security_requirements(openapi: &OpenApi, config: &OpenApiConfig) -> crate::Result<()> {
    let value = serde_json::to_value(openapi).map_err(|error| {
        openapi_configuration_error(format!(
            "failed to serialize OpenAPI security requirements: {error}"
        ))
    })?;
    let Some(paths) = value.get("paths").and_then(serde_json::Value::as_object) else {
        return Ok(());
    };
    for (path, item) in paths {
        let Some(item) = item.as_object() else {
            continue;
        };
        for (method, operation) in item {
            let Some(requirements) = operation
                .get("security")
                .and_then(serde_json::Value::as_array)
            else {
                continue;
            };
            for requirement in requirements {
                let Some(requirement) = requirement.as_object() else {
                    continue;
                };
                for name in requirement.keys() {
                    let Some(configured) = config.security_schemes.get(name) else {
                        return Err(openapi_configuration_error(format!(
                            "OpenAPI security requirement `{name}` on {method} {path} has no registered scheme"
                        )));
                    };
                    let expected = match name.as_str() {
                        "BearerAuth" => Some(ConfiguredSecurityKind::BearerAuth),
                        "ApiKeyAuth" => Some(ConfiguredSecurityKind::ApiKeyAuth),
                        "CookieAuth" => Some(ConfiguredSecurityKind::CookieAuth),
                        "OAuth2" => Some(ConfiguredSecurityKind::OAuth2),
                        _ => None,
                    };
                    if expected.is_some_and(|expected| configured.kind != expected) {
                        return Err(openapi_configuration_error(format!(
                            "OpenAPI security requirement `{name}` on {method} {path} does not match its registered scheme"
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_document_paths(openapi: &OpenApi, config: &OpenApiConfig) -> crate::Result<()> {
    if openapi.paths.paths.keys().any(|path| {
        path == &config.json_path
            || path == &config.ui_path
            || path.starts_with(&format!("{}/", config.ui_path.trim_end_matches('/')))
    }) {
        return Err(openapi_configuration_error(
            "OpenAPI paths must not collide with controller routes",
        ));
    }
    Ok(())
}

fn openapi_configuration_error(message: impl Into<String>) -> HttpException {
    HttpException::new(
        StatusCode::INTERNAL_SERVER_ERROR,
        "OpenAPI Configuration Error",
        message,
    )
}

/// Adds one combined (OpenAPI AND) security requirement to an operation.
#[doc(hidden)]
pub fn apply_security(operation: &mut Operation, security: &[Security]) {
    if security.is_empty() {
        return;
    }
    let mut requirement = security::SecurityRequirement::default();
    for security in security {
        let (name, scopes) = security.name_and_scopes();
        requirement = requirement.add(name, scopes.iter().copied());
    }
    operation.security = Some(vec![requirement]);
}

/// Registers all schemas referenced by `T` and returns its component reference.
#[doc(hidden)]
pub fn schema_ref<T: ToSchema>(openapi: &mut OpenApi) -> RefOr<Schema> {
    add_schema::<T>(openapi);
    Ref::from_schema_name(T::name()).into()
}

/// Returns a schema suitable for primitive parameter/header values.
#[doc(hidden)]
pub fn inline_schema<T: PartialSchema>() -> RefOr<Schema> {
    T::schema()
}

#[doc(hidden)]
pub fn operation(method: &str, path: &str, operation: Operation, openapi: &mut OpenApi) {
    let method = match method {
        "get" => HttpMethod::Get,
        "post" => HttpMethod::Post,
        "put" => HttpMethod::Put,
        "patch" => HttpMethod::Patch,
        "delete" => HttpMethod::Delete,
        _ => return,
    };
    openapi
        .paths
        .add_path_operation(path, vec![method], operation);
}

#[doc(hidden)]
pub fn parameter(
    name: impl Into<String>,
    parameter_in: ParameterIn,
    required: bool,
    description: Option<&str>,
    schema: RefOr<Schema>,
) -> Parameter {
    let mut parameter = Parameter::new(name);
    parameter.parameter_in = parameter_in;
    parameter.description = description.map(str::to_owned);
    parameter.required = if required {
        Required::True
    } else {
        Required::False
    };
    if parameter.parameter_in == ParameterIn::Path {
        parameter.required = Required::True;
    }
    parameter.schema = Some(schema);
    parameter
}

#[doc(hidden)]
pub fn request_body(schema: RefOr<Schema>) -> RequestBody {
    let mut request = RequestBody::new();
    request.required = Some(Required::True);
    request
        .content
        .insert("application/json".into(), Content::new(Some(schema)));
    request
}

/// Builds a multipart request body from an optional DTO schema and named file
/// fields. It is used by the controller macro when a route accepts uploads.
#[doc(hidden)]
pub fn multipart_request_body(
    dto_schema: Option<RefOr<Schema>>,
    files: &[(&str, bool, bool)],
) -> RequestBody {
    let mut file_properties = ObjectBuilder::new();
    for (name, repeated, required) in files {
        let binary = ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
            .build();
        let schema: RefOr<Schema> = if *repeated {
            ArrayBuilder::new().items(binary).build().into()
        } else {
            binary.into()
        };
        file_properties = file_properties.property(*name, schema);
        if *required {
            file_properties = file_properties.required(*name);
        }
    }
    let file_properties: Schema = file_properties.build().into();
    let schema: RefOr<Schema> = match dto_schema {
        Some(dto_schema) => Schema::AllOf(
            AllOfBuilder::new()
                .item(dto_schema)
                .item(file_properties)
                .build(),
        )
        .into(),
        None => file_properties.into(),
    };
    let mut request = RequestBody::new();
    request.required = Some(Required::True);
    request
        .content
        .insert("multipart/form-data".into(), Content::new(Some(schema)));
    request
}

#[doc(hidden)]
pub fn response(content_type: Option<&str>, schema: Option<RefOr<Schema>>) -> Response {
    let mut response = Response::new("Successful response");
    if let Some(content_type) = content_type {
        response
            .content
            .insert(content_type.into(), Content::new(schema));
    }
    response
}

#[doc(hidden)]
pub fn error_response<E: OpenApiError>(openapi: &mut OpenApi) -> (String, Response) {
    let schema = schema_ref::<ErrorEnvelope>(openapi);
    let mut response = Response::new(E::description());
    response
        .content
        .insert("application/json".into(), Content::new(Some(schema)));
    (E::status().as_u16().to_string(), response)
}

#[doc(hidden)]
pub fn add_schema<T: ToSchema>(openapi: &mut OpenApi) {
    let components = openapi.components.get_or_insert_with(Components::new);
    let mut schemas = vec![(T::name().into_owned(), T::schema())];
    T::schemas(&mut schemas);
    for (name, schema) in schemas {
        components.schemas.entry(name).or_insert(schema);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unregistered_route_security_requirements() {
        let config = OpenApiConfig::new("Test", "1.0.0");
        let mut openapi = OpenApi::new(Info::new("Test", "1.0.0"), utoipa::openapi::Paths::new());
        let mut route_operation = Operation::new();
        apply_security(&mut route_operation, &[Security::BearerAuth]);
        operation("get", "/protected", route_operation, &mut openapi);

        let error = validate_security_requirements(&openapi, &config).unwrap_err();
        assert!(error.message.contains("has no registered scheme"));
    }
}
