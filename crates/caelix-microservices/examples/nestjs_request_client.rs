use caelix_microservices::{MicroserviceClient, NatsTransportOptions};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Serialize)]
struct EchoRequest {
    value: String,
}

#[derive(Debug, Deserialize)]
struct EchoResponse {
    value: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = MicroserviceClient::connect(
        NatsTransportOptions::new("nats://127.0.0.1:4222")
            .service_name("caelix-interop-client")
            .rpc_timeout(Duration::from_secs(3)),
    )
    .await?;
    let response: EchoResponse = client
        .request(
            "interop.echo",
            EchoRequest {
                value: "hello".into(),
            },
        )
        .await?;
    assert_eq!(response.value, "nest:hello");
    println!("Caelix → NestJS NATS request/reply succeeded");
    Ok(())
}
