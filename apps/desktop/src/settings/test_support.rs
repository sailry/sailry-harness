//! A transport-free binding for controller request-state tests.
use super::providers::Binding;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Fault, NodeId, Request, Topic};
use std::sync::Arc;

struct Offline(NodeId);

impl Transport for Offline {
    fn target(&self) -> NodeId {
        self.0
    }
    fn dispatch(&self, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        panic!("request admission must not perform transport I/O")
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        panic!("request admission must not create subscriptions")
    }
}

pub(super) fn binding(node: NodeId) -> Binding {
    Binding {
        client: Arc::new(sailry_client::Client::new(Arc::new(Offline(node)))),
        runtime: Arc::new(
            tokio::runtime::Builder::new_current_thread()
                .build()
                .unwrap(),
        ),
        label: "Fixture".into(),
    }
}
