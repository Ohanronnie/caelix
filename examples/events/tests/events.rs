use caelix::prelude::*;
use events::{AuditLog, UserCreatedEvent, UserEventsModule};

#[test]
fn example_module_registers_and_runs_multiple_handlers() {
    let container = events::block_on(build_container::<UserEventsModule>());
    let bus = container.resolve::<EventBus>();

    assert_eq!(bus.handler_count::<UserCreatedEvent>(), 2);

    events::block_on(bus.emit(UserCreatedEvent {
        user_id: 7,
        email: "ronnie@example.com".to_string(),
    }))
    .unwrap();

    assert_eq!(
        container.resolve::<AuditLog>().entries(),
        vec!["welcome-email:ronnie@example.com", "audit-log:7"]
    );
}
