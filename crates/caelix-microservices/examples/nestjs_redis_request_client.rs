use caelix_microservices::{MicroserviceClient, RedisTransportOptions};
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
    let redis_url =
        std::env::var("CAELIX_REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379/".into());
    let client = MicroserviceClient::connect(
        RedisTransportOptions::new(redis_url)
            .service_name("caelix-interop-client")
            .rpc_timeout(Duration::from_secs(5)),
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
    println!("Caelix → NestJS Redis request/reply succeeded");
    Ok(())
}
