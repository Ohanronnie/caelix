use caelix::prelude::*;

#[test]
fn public_prelude_reexports_core_response_types() {
    let response = Response::text(StatusCode::CREATED, "made").into_response();

    assert_eq!(response.status, StatusCode::CREATED);
    assert_eq!(response.content_type, "text/plain");
    assert_eq!(response.body, b"made");
}
