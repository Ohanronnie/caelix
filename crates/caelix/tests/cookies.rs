#![cfg(any(feature = "actix", feature = "axum"))]

#[cfg(feature = "openapi")]
use caelix::Controller;
use caelix::{
    BoxFuture, Container, Injectable, Module, ModuleMetadata, Response, Result, StatusCode,
    TestApplication, controller,
};

struct CookieController;

impl Injectable for CookieController {
    fn dependencies() -> Vec<caelix::ProviderDependency> {
        caelix::provider_dependencies![]
    }

    fn create(_: &Container) -> BoxFuture<'_, Result<Self>> {
        Box::pin(async { Ok(Self) })
    }
}

#[cfg(feature = "openapi")]
#[test]
fn cookie_extractors_generate_required_and_optional_openapi_parameters() {
    use caelix::openapi::utoipa::openapi::{Info, OpenApi, Paths};

    let mut document = OpenApi::new(Info::new("Cookies", "1.0.0"), Paths::new());
    for route in CookieController::openapi_routes() {
        (route.document)(&mut document);
    }
    let value = serde_json::to_value(document).unwrap();
    let required = &value["paths"]["/cookies/required"]["get"]["parameters"][0];
    assert_eq!(required["name"], "session");
    assert_eq!(required["in"], "cookie");
    assert_eq!(required["required"], true);
    assert_eq!(required["schema"]["type"], "string");

    let optional = &value["paths"]["/cookies/optional"]["get"]["parameters"][0];
    assert_eq!(optional["name"], "preference");
    assert_eq!(optional["in"], "cookie");
    assert_eq!(optional["required"], false);
    assert_eq!(optional["schema"]["type"], "string");
}

#[controller("/cookies")]
impl CookieController {
    #[get("/required")]
    async fn required(&self, #[cookie("session")] session: String) -> Result<Response<String>> {
        Ok(Response::Body(session))
    }

    #[get("/optional")]
    async fn optional(
        &self,
        #[cookie("preference")] preference: Option<String>,
    ) -> Result<Response<Option<String>>> {
        Ok(Response::Body(preference))
    }
}

struct CookieModule;

impl Module for CookieModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<CookieController>()
    }
}

#[caelix::test]
async fn generated_cookie_extractors_are_runtime_neutral() {
    let app = TestApplication::new::<CookieModule>().await.unwrap();

    let value: String = app
        .get("/cookies/required")
        .header("cookie", "other=x; session=hello%20world")
        .send()
        .await
        .unwrap()
        .json()
        .await;
    assert_eq!(value, "hello world");

    let value: Option<String> = app
        .get("/cookies/optional")
        .send()
        .await
        .unwrap()
        .json()
        .await;
    assert_eq!(value, None);

    let error: serde_json::Value = app
        .get("/cookies/required")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::BAD_REQUEST)
        .json()
        .await;
    assert_eq!(error["message"], "missing required cookie 'session'");
}
