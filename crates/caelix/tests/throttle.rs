use caelix::TestApplication;
use caelix::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

static GUARD_CALLS: AtomicUsize = AtomicUsize::new(0);
static CONTROLLER_CALLS: AtomicUsize = AtomicUsize::new(0);

#[injectable]
struct CountingGuard;

impl Guard for CountingGuard {
    fn can_activate<'a>(&'a self, _: &'a RequestContext) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async {
            GUARD_CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    }
}

#[injectable]
struct ThrottleController;

#[controller("/throttle")]
#[throttle(limit = 2, window_seconds = 60)]
impl ThrottleController {
    #[get("/limited")]
    async fn limited(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("ok"))
    }

    #[get("/other")]
    async fn other(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("other"))
    }

    #[get("/override")]
    #[throttle(limit = 1, window_seconds = 60)]
    async fn overridden(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("override"))
    }

    #[get("/open")]
    #[skip_throttle]
    async fn open(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("open"))
    }

    #[post("/body")]
    #[throttle(limit = 1, window_seconds = 60)]
    async fn body(&self, #[body] _body: String) -> Result<Response<&'static str>> {
        Ok(Response::Body("body"))
    }

    #[get("/guarded")]
    #[throttle(limit = 1, window_seconds = 60)]
    #[use_guard(CountingGuard)]
    async fn guarded(&self) -> Result<Response<&'static str>> {
        CONTROLLER_CALLS.fetch_add(1, Ordering::SeqCst);
        Ok(Response::Body("guarded"))
    }
}

#[injectable]
struct GlobalController;

#[controller("/global")]
impl GlobalController {
    #[get("")]
    async fn global(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("global"))
    }
}

#[injectable]
struct SkippedController;

#[caelix::skip_throttle]
#[controller("/skipped")]
impl SkippedController {
    #[get("/enabled")]
    #[caelix::throttle(limit = 1, window_seconds = 60)]
    async fn enabled(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("enabled"))
    }
}

struct TestThrottleConfig;

#[injectable]
struct TrackerIdentity;

struct TrackerDependencyModule;

impl Module for TrackerDependencyModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<TrackerIdentity>()
            .export::<TrackerIdentity>()
    }
}

struct InjectedTracker {
    _identity: Arc<TrackerIdentity>,
}

impl ThrottleTracker for InjectedTracker {
    fn track<'a>(&'a self, _: &'a RequestContext) -> BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("injected-test-client".to_string()) })
    }
}

impl ThrottleConfig for TestThrottleConfig {
    fn imports() -> Vec<ModuleDef> {
        vec![ModuleDef::of::<TrackerDependencyModule>()]
    }

    fn dependencies() -> Vec<ProviderDependency> {
        vec![ProviderDependency::of::<TrackerIdentity>()]
    }

    fn options(container: &Container) -> Result<ThrottleOptions> {
        let identity = container.resolve::<TrackerIdentity>()?;
        Ok(ThrottleOptions {
            policy: ThrottlePolicy::new(2, 60),
            ..ThrottleOptions::default()
        }
        .with_tracker(Arc::new(InjectedTracker {
            _identity: identity,
        })))
    }
}

struct AppModule;

impl Module for AppModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ThrottleModule<TestThrottleConfig>>()
            .controller::<ThrottleController>()
            .controller::<GlobalController>()
            .controller::<SkippedController>()
            .provider::<CountingGuard>()
    }
}

#[injectable]
struct OverflowController;

#[controller("/overflow")]
impl OverflowController {
    #[get("")]
    #[throttle(limit = 1, window_seconds = 18446744073709551615)]
    async fn get(&self) -> Result<Response<&'static str>> {
        Ok(Response::Body("never"))
    }
}

struct OverflowModule;

impl Module for OverflowModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .import::<ThrottleModule>()
            .controller::<OverflowController>()
    }
}

#[caelix::test]
async fn rejects_unrepresentable_explicit_policy_at_startup() {
    let error = match TestApplication::new::<OverflowModule>().await {
        Ok(_) => panic!("overflowing throttle policy unexpectedly started"),
        Err(error) => error,
    };
    assert!(error.message.contains("clock range"));
}

#[caelix::test]
async fn enforces_overrides_isolation_and_skip() {
    GUARD_CALLS.store(0, Ordering::SeqCst);
    CONTROLLER_CALLS.store(0, Ordering::SeqCst);
    let app = TestApplication::new::<AppModule>().await.unwrap();

    app.get("/throttle/limited")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    app.get("/throttle/limited")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    let rejected = app
        .get("/throttle/limited")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
    assert!(rejected.header("retry-after").is_some());

    #[derive(serde::Deserialize)]
    struct ErrorBody {
        message: String,
    }
    assert_eq!(
        rejected.json::<ErrorBody>().await.message,
        "Rate limit exceeded"
    );

    app.post("/throttle/body")
        .json("ok")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    app.post("/throttle/body")
        .set_payload(vec![b'x'; 1024 * 1024 + 1])
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::TOO_MANY_REQUESTS);

    app.get("/throttle/guarded")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    app.get("/throttle/guarded")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(GUARD_CALLS.load(Ordering::SeqCst), 1);
    assert_eq!(CONTROLLER_CALLS.load(Ordering::SeqCst), 1);

    app.get("/throttle/other")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    app.get("/throttle/override")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    app.get("/throttle/override")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
    for _ in 0..4 {
        app.get("/throttle/open")
            .send()
            .await
            .unwrap()
            .assert_status(StatusCode::OK);
    }

    for expected in [
        StatusCode::OK,
        StatusCode::OK,
        StatusCode::TOO_MANY_REQUESTS,
    ] {
        app.get("/global")
            .send()
            .await
            .unwrap()
            .assert_status(expected);
    }

    app.get("/skipped/enabled")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::OK);
    app.get("/skipped/enabled")
        .send()
        .await
        .unwrap()
        .assert_status(StatusCode::TOO_MANY_REQUESTS);
}
