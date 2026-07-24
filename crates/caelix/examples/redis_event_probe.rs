use caelix::{
    MicroserviceApplication, MicroserviceClient, Module, ModuleMetadata, RedisTransportOptions,
    Result,
};
use serde_json::Value;

#[caelix::injectable]
struct RedisProbe;

#[caelix::microservice]
impl RedisProbe {
    #[caelix::event_pattern("caelix.probe")]
    async fn receive(&self, #[caelix::payload] payload: Value) -> Result<()> {
        println!("received Redis Stream event: {payload}");
        Ok(())
    }
}

struct ProbeModule;

impl Module for ProbeModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().microservice::<RedisProbe>()
    }
}

#[caelix::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let server = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let options = RedisTransportOptions::new(server)
        .service_name("caelix-redis-probe")
        .event_stream("caelix:events");
    if std::env::args().nth(1).as_deref() == Some("emit") {
        MicroserviceClient::connect(options)
            .await?
            .emit(
                "caelix.probe",
                serde_json::json!({"source": "caelix", "transport": "redis"}),
            )
            .await?;
        return Ok(());
    }
    MicroserviceApplication::<ProbeModule>::new(options)
        .await?
        .run()
        .await?;
    Ok(())
}
