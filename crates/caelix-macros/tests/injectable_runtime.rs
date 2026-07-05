use std::{future::Future, sync::Arc};

use caelix_core::{Container, Logger};
use caelix_macros::injectable;

fn block_on<F: Future>(future: F) -> F::Output {
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

#[injectable]
struct Service {
    logger: Arc<Logger>,
}

#[test]
fn injectable_macro_injects_contextual_logger() {
    let mut container = Container::new();

    block_on(container.register::<Service>());
    let service = container.resolve::<Service>();

    assert_eq!(service.logger.context(), "Service");
}
