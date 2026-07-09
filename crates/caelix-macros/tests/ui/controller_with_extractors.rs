use caelix_core::{
    BoxFuture, Container, Guard, HttpResponse, Interceptor, Next, RequestContext, Result,
};
use caelix_macros::{controller, guard, injectable};
use serde::Deserialize;

mod caelix {
    pub use caelix_actix::{__actix_web, to_actix_response};
    pub use caelix_core::*;
}

#[derive(Deserialize)]
struct SearchQuery {
    term: String,
}

#[derive(Deserialize)]
struct CreateUser {
    name: String,
}

#[derive(Clone)]
struct CurrentUser {
    id: i64,
}

#[guard]
struct AuthGuard;

impl Guard for AuthGuard {
    fn can_activate<'a>(&'a self, ctx: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            ctx.set(CurrentUser { id: 42 })?;
            Ok(true)
        })
    }
}

#[injectable]
struct AuditInterceptor;

impl Interceptor for AuditInterceptor {
    fn intercept<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<HttpResponse>> {
        Box::pin(async move { next.run().await })
    }
}

#[injectable]
struct UserController;

#[controller("/users")]
#[use_guard(AuthGuard)]
impl UserController {
    #[get("/{id}")]
    async fn get_user(&self, #[param] id: i64) -> Result<String> {
        Ok(id.to_string())
    }

    #[get("/")]
    async fn search_users(&self, #[query] query: SearchQuery) -> Result<String> {
        Ok(query.term)
    }

    #[post("/")]
    #[use_interceptor(AuditInterceptor)]
    async fn create_user(&self, #[body] body: CreateUser) -> Result<String> {
        Ok(body.name)
    }

    #[get("/me")]
    async fn me(&self, #[user] user: CurrentUser) -> Result<String> {
        Ok(user.id.to_string())
    }
}

async fn exercise() {
    let mut container = Container::new();
    container.register::<AuthGuard>().await.unwrap();
    container.register::<AuditInterceptor>().await.unwrap();
    container.register::<UserController>().await.unwrap();

    let _controller = container.resolve::<UserController>().unwrap();
}

fn main() {
    std::mem::drop(exercise());
}
