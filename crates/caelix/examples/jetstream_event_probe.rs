use caelix::{
    MicroserviceApplication, MicroserviceClient, Module, ModuleMetadata, NatsTransportOptions,
    Result,
};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

static DELIVERIES: AtomicUsize = AtomicUsize::new(0);

#[caelix::injectable]
struct EventProbe;

#[caelix::microservice]
impl EventProbe {
    #[caelix::event_pattern("interop.probe")]
    async fn probe(&self, #[caelix::payload] _value: serde_json::Value) -> Result<()> {
        DELIVERIES.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct EventModule;

impl Module for EventModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().microservice::<EventProbe>()
    }
}

#[caelix::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let options = NatsTransportOptions::new("nats://127.0.0.1:4222")
        .service_name("caelix-event-probe")
        .jetstream_stream("CAELIX_EVENT_PROBE")
        .max_event_deliveries(3);
    let application = MicroserviceApplication::<EventModule>::new(options.clone()).await?;
    let runtime = caelix::__tokio::spawn(application.run());
    caelix::__tokio::time::sleep(Duration::from_millis(100)).await;
    MicroserviceClient::connect(options)
        .await?
        .emit("interop.probe", serde_json::json!({"ok": true}))
        .await?;
    for _ in 0..30 {
        if DELIVERIES.load(Ordering::SeqCst) == 1 {
            runtime.abort();
            println!("Caelix JetStream event delivery succeeded");
            return Ok(());
        }
        caelix::__tokio::time::sleep(Duration::from_millis(100)).await;
    }
    runtime.abort();
    Err("JetStream event was not delivered".into())
}
