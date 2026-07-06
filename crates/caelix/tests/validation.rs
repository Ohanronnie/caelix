#![cfg(all(feature = "actix", feature = "validator"))]

use actix_web::{App, test as actix_test, web};
use caelix::{ModuleMetadata, Response, Result, controller, injectable};
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
struct CreateUserBody {
    #[validate(required)]
    name: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
struct SearchUsersQuery {
    #[validate(required)]
    q: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
struct UserPath {
    #[validate(range(min = 1))]
    id: i64,
}

#[injectable]
struct ValidationController;

#[controller("/validation")]
impl ValidationController {
    #[post("/body")]
    async fn create_body(
        &self,
        #[body]
        #[validate]
        _body: CreateUserBody,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"ok": true})))
    }

    #[get("/query")]
    async fn search_query(
        &self,
        #[query]
        #[validate]
        _query: SearchUsersQuery,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"ok": true})))
    }

    #[get("/path/{id}")]
    async fn find_by_path(
        &self,
        #[param]
        #[validate]
        _path: UserPath,
    ) -> Result<Response<Value>> {
        Ok(Response::Body(json!({"ok": true})))
    }
}

struct ValidationModule;

impl caelix::Module for ValidationModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().controller::<ValidationController>()
    }
}

#[actix_web::test]
async fn validate_body_empty_object_returns_field_errors() {
    let container = std::sync::Arc::new(caelix::build_container::<ValidationModule>().await);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(container))
            .configure(|cfg| caelix::register_module_controllers::<ValidationModule>(cfg)),
    )
    .await;

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::post()
            .uri("/validation/body")
            .insert_header(("content-type", "application/json"))
            .set_payload("{}")
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), actix_web::http::StatusCode::BAD_REQUEST);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(
        body,
        json!({
            "status": 400,
            "error": "Bad Request",
            "message": "Validation failed",
            "errors": {
                "name": ["is required"]
            }
        })
    );
}

#[actix_web::test]
async fn validate_query_empty_query_returns_field_errors() {
    let container = std::sync::Arc::new(caelix::build_container::<ValidationModule>().await);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(container))
            .configure(|cfg| caelix::register_module_controllers::<ValidationModule>(cfg)),
    )
    .await;

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/validation/query")
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), actix_web::http::StatusCode::BAD_REQUEST);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(
        body,
        json!({
            "status": 400,
            "error": "Bad Request",
            "message": "Validation failed",
            "errors": {
                "q": ["is required"]
            }
        })
    );
}

#[actix_web::test]
async fn validate_path_runs_after_path_deserialization() {
    let container = std::sync::Arc::new(caelix::build_container::<ValidationModule>().await);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(container))
            .configure(|cfg| caelix::register_module_controllers::<ValidationModule>(cfg)),
    )
    .await;

    let response = actix_test::call_service(
        &app,
        actix_test::TestRequest::get()
            .uri("/validation/path/0")
            .to_request(),
    )
    .await;

    assert_eq!(response.status(), actix_web::http::StatusCode::BAD_REQUEST);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(
        body,
        json!({
            "status": 400,
            "error": "Bad Request",
            "message": "Validation failed",
            "errors": {
                "id": ["is invalid (range)"]
            }
        })
    );
}
