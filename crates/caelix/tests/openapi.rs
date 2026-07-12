#![cfg(feature = "openapi")]

use caelix::openapi::{
    OpenApiConfig, Security, ToSchema, errors, request_header, response, security, utoipa,
};
use caelix::{
    BadRequestException, ConflictException, Module, ModuleMetadata, Response, Result,
    TestApplication, controller, injectable,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
struct PaymentDto {
    id: String,
}

#[injectable]
struct DocumentationController;

#[controller("/payments")]
impl DocumentationController {
    #[post("")]
    #[request_header(name = "Idempotency-Key", schema = String, required, description = "Safe retry key")]
    #[request_header(name = "X-API-Key", schema = String, required)]
    #[response(status = 201, body = PaymentDto, headers(("Location", String, "Payment URL")))]
    #[errors(BadRequestException, ConflictException)]
    #[security(Security::BearerAuth)]
    #[security(Security::OAuth2(&["users:read"]))]
    #[security(Security::Custom { name: "TenantAuth", scopes: &[] })]
    async fn create(&self, #[body] payment: PaymentDto) -> Result<Response<PaymentDto>> {
        Ok(Response::WithStatus(caelix::StatusCode::CREATED, payment))
    }

    #[get("/report")]
    #[response(content_type = "application/pdf", headers(("Content-Disposition", String)))]
    async fn report(&self) -> Result<Response<()>> {
        Ok(Response::bytes(caelix::StatusCode::OK, b"pdf".to_vec()))
    }
}

struct DocumentationModule;

impl Module for DocumentationModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<DocumentationController>()
    }
}

#[caelix::test]
async fn serves_openapi_documentation_with_route_metadata() {
    let app = TestApplication::new::<DocumentationModule>()
        .with_openapi(
            OpenApiConfig::new("Payments", "1.0.0")
                .bearer_auth()
                .api_key_auth("X-API-Key")
                .cookie_auth("session")
                .oauth2(security::OAuth2::new([security::Flow::ClientCredentials(
                    security::ClientCredentials::new(
                        "https://auth.example.test/token",
                        security::Scopes::from_iter([("users:read", "Read users")]),
                    ),
                )]))
                .security_scheme(
                    "TenantAuth",
                    security::SecurityScheme::ApiKey(security::ApiKey::Header(
                        security::ApiKeyValue::new("X-Tenant"),
                    )),
                ),
        )
        .await
        .unwrap();

    let document: Value = app.get("/openapi.json").send().await.unwrap().json().await;
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["info"]["title"], "Payments");
    let operation = &document["paths"]["/payments"]["post"];
    assert_eq!(operation["requestBody"]["required"], true);
    assert_eq!(operation["parameters"][0]["in"], "header");
    assert_eq!(operation["parameters"][0]["required"], true);
    assert_eq!(
        operation["responses"]["201"]["headers"]["Location"]["description"],
        "Payment URL"
    );
    assert!(operation["responses"].get("400").is_some());
    assert!(operation["responses"].get("409").is_some());
    assert_eq!(
        operation["security"][0]["BearerAuth"],
        serde_json::json!([])
    );
    assert_eq!(
        operation["security"][0]["OAuth2"],
        serde_json::json!(["users:read"])
    );
    assert_eq!(
        operation["security"][0]["TenantAuth"],
        serde_json::json!([])
    );
    assert!(
        document["components"]["schemas"]
            .get("PaymentDto")
            .is_some()
    );
    assert_eq!(
        document["components"]["securitySchemes"]["BearerAuth"]["scheme"],
        "bearer"
    );
    assert_eq!(
        document["components"]["securitySchemes"]["ApiKeyAuth"]["in"],
        "header"
    );
    assert_eq!(
        document["components"]["securitySchemes"]["CookieAuth"]["in"],
        "cookie"
    );
    assert!(
        document["components"]["securitySchemes"]
            .get("OAuth2")
            .is_some()
    );
    assert!(
        document["components"]["securitySchemes"]
            .get("TenantAuth")
            .is_some()
    );
    assert!(
        document["components"]["schemas"]
            .get("ErrorEnvelope")
            .is_some()
    );

    let report = &document["paths"]["/payments/report"]["get"]["responses"]["200"];
    assert!(report["content"]["application/pdf"]["schema"].is_null());
    assert!(report["headers"].get("Content-Disposition").is_some());
    app.get("/docs/")
        .send()
        .await
        .unwrap()
        .assert_status(caelix::StatusCode::OK);
}
