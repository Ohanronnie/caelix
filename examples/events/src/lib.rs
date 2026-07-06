use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use caelix::prelude::*;

#[derive(Clone)]
pub struct UserCreatedEvent {
    pub user_id: i64,
    pub email: String,
}

pub struct AuditLog {
    entries: Mutex<Vec<String>>,
}

impl AuditLog {
    pub fn entries(&self) -> Vec<String> {
        self.entries.lock().unwrap().clone()
    }

    fn push(&self, entry: String) {
        self.entries.lock().unwrap().push(entry);
    }
}

impl Injectable for AuditLog {
    fn create(_container: &Container) -> BoxFuture<'_, Self> {
        Box::pin(async {
            Self {
                entries: Mutex::new(Vec::new()),
            }
        })
    }
}

#[injectable]
pub struct SendWelcomeEmail {
    audit_log: Arc<AuditLog>,
}

impl EventHandler<UserCreatedEvent> for SendWelcomeEmail {
    fn handle(&self, event: UserCreatedEvent) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.audit_log
                .push(format!("welcome-email:{}", event.email));
            Ok(())
        })
    }
}

impl RegisterableEventHandler for SendWelcomeEmail {
    type Event = UserCreatedEvent;
}

#[injectable]
pub struct LogUserCreated {
    audit_log: Arc<AuditLog>,
}

impl EventHandler<UserCreatedEvent> for LogUserCreated {
    fn handle(&self, event: UserCreatedEvent) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.audit_log.push(format!("audit-log:{}", event.user_id));
            Ok(())
        })
    }
}

pub struct UserEventsModule;

impl Module for UserEventsModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new()
            .provider::<AuditLog>()
            .provider::<SendWelcomeEmail>()
            .provider::<LogUserCreated>()
            .event_handler::<SendWelcomeEmail>()
            .event_handler_for::<UserCreatedEvent, LogUserCreated>()
    }
}

pub async fn emit_example_event() -> Result<Vec<String>> {
    let container = build_container::<UserEventsModule>().await;
    let bus = container.resolve::<EventBus>();

    bus.emit(UserCreatedEvent {
        user_id: 7,
        email: "ronnie@example.com".to_string(),
    })
    .await?;

    Ok(container.resolve::<AuditLog>().entries())
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
