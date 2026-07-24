#![cfg(feature = "microservices-nats")]

use caelix::{MessageContext, Microservice, Module, ModuleMetadata, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
struct Echo {
    value: String,
}

#[caelix::injectable]
struct EchoMicroservice;

#[caelix::microservice]
impl EchoMicroservice {
    #[caelix::message_pattern("test.echo")]
    async fn echo(&self, #[caelix::payload] input: Echo) -> Result<Echo> {
        Ok(input)
    }

    #[caelix::event_pattern("test.logged")]
    async fn logged(
        &self,
        #[caelix::context] _context: MessageContext,
        #[caelix::payload] _input: Echo,
    ) -> Result<()> {
        Ok(())
    }
}

struct MicroserviceModule;

impl Module for MicroserviceModule {
    fn register() -> ModuleMetadata {
        ModuleMetadata::new().microservice::<EchoMicroservice>()
    }
}

#[caelix::test]
async fn a_microservice_is_a_normal_provider_with_typed_handlers() {
    let container = caelix::build_container::<MicroserviceModule>()
        .await
        .unwrap();
    assert!(container.resolve::<EchoMicroservice>().is_ok());
    assert_eq!(EchoMicroservice::definition().handlers().len(), 2);
}
