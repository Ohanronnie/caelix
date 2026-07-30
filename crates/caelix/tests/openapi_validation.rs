#![cfg(all(feature = "openapi", feature = "validator"))]

use caelix::openapi::{OpenApiConfig, ToSchema, response, utoipa};
use caelix::{
    Module, ModuleMetadata, Response, Result, TestApplication, controller, injectable,
    validator::Validate,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Deserialize, Serialize, ToSchema, Validate)]
struct ValidatedBody {
    #[validate(length(min = 1))]
    email: String,
}

#[derive(Deserialize, ToSchema, Validate)]
struct ValidatedQuery {
    #[validate(length(min = 1))]
    search: String,
}

#[injectable]
struct ValidatedDocumentationController;

#[controller("/validated-docs")]
impl ValidatedDocumentationController {
    #[post("/body")]
    #[response(body = ValidatedBody)]
    async fn body(
        &self,
        #[body]
        #[validate]
        input: ValidatedBody,
    ) -> Result<Response<ValidatedBody>> {
        Ok(Response::Body(input))
    }

    #[get("/query")]
    #[response(body = ValidatedBody)]
    async fn query(
        &self,
        #[query]
        #[validate]
        query: ValidatedQuery,
    ) -> Result<Response<ValidatedBody>> {
        Ok(Response::Body(ValidatedBody {
            email: query.search,
        }))
    }
}

struct ValidatedDocumentationModule;

impl Module for ValidatedDocumentationModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<ValidatedDocumentationController>()
    }
}

#[caelix::test]
async fn validated_extractors_keep_their_openapi_schema_fields() {
    let app = TestApplication::new::<ValidatedDocumentationModule>()
        .with_openapi(OpenApiConfig::new("Validation", "1.0.0"))
        .await
        .unwrap();

    let document: Value = app.get("/openapi.json").send().await.unwrap().json().await;
    let body = &document["paths"]["/validated-docs/body"]["post"];
    let query = &document["paths"]["/validated-docs/query"]["get"];

    assert_eq!(
        body["requestBody"]["content"]["application/json"]["schema"]["$ref"],
        "#/components/schemas/ValidatedBody"
    );
    assert!(
        document["components"]["schemas"]["ValidatedBody"]["properties"]
            .get("email")
            .is_some()
    );
    assert!(
        query["parameters"][0]["schema"]["properties"]
            .get("search")
            .is_some()
    );
}
