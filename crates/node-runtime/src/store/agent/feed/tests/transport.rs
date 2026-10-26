use super::*;
use sailry_link::{
    Admission, EndpointAddr, Handler, Identity, Link, Local, NetworkScope, PeerStore, Transport,
};
use sailry_protocol::{Request, Topic};

struct Service(Mutex<Option<Box<dyn Subscription>>>);

impl Handler for Service {
    fn dispatch(&self, _: NodeId, _: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async { Err(Fault::invalid("fixture only serves conversation updates")) })
    }

    fn subscribe(&self, _: NodeId, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async {
            self.0
                .lock()
                .unwrap()
                .take()
                .ok_or_else(crate::store::unavailable)
        })
    }
}

struct Peers;

impl PeerStore for Peers {
    fn peers(&self) -> Pending<'_, Result<Vec<NodeId>, Fault>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn addresses(&self) -> Pending<'_, Result<Vec<EndpointAddr>, Fault>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn remember(&self, _: EndpointAddr) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
    fn set_trust(&self, _: NodeId, _: bool) -> Pending<'_, Result<(), Fault>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn resumes_gap_on_both_transports() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let identity = Identity::open(directory.path()).unwrap();
        let node = identity.id();
        let (feeds, _, session, mut draft) = streaming();
        draft.parts = vec![Part::Text("x".repeat(4096))];
        let history = History {
            sequence: 0,
            page: Page {
                session,
                revision: 1,
                entries: Vec::new(),
                runs: Vec::new(),
                next_before: None,
                approvals: Vec::new(),
                questions: Vec::new(),
                children: Vec::new(),
                background: Vec::new(),
                queue: Default::default(),
            },
            missing: Vec::new(),
        };
        let subscription = feeds
            .subscribe(
                node,
                history,
                Default::default(),
                Default::default(),
                CancellationToken::new(),
            )
            .unwrap();
        // Create a real broadcast gap before either adapter starts draining it.
        for _ in 0..100 {
            feeds.delta(node, session, draft.clone()).unwrap();
        }
        let service = Arc::new(Service(Mutex::new(Some(subscription))));
        let host = Link::bind(
            &identity,
            NetworkScope::default(),
            service.clone(),
            Arc::new(Peers),
        )
        .await
        .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let caller = NodeId(*controller.handle().address().id.as_bytes());
        controller.handle().set_trust(node, true).await.unwrap();
        host.handle().set_trust(caller, true).await.unwrap();
        let transport: Arc<dyn Transport> = if remote {
            controller.handle().remote(host.handle().address())
        } else {
            Arc::new(Local::new(node, caller, service))
        };
        let mut updates = transport
            .subscribe(Topic::Conversation(session))
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let Update::ConversationSnapshot(snapshot) = updates.next().await.unwrap() else {
                panic!("snapshot expected")
            };
            assert_eq!(snapshot.sequence, 0);
            for sequence in 1..=101 {
                let Update::ConversationFrame(frame) = updates.next().await.unwrap() else {
                    panic!("replayed frame expected")
                };
                assert_eq!(
                    (frame.node, frame.session, frame.sequence),
                    (node, session, sequence)
                );
                assert_eq!(frame.change, Change::Delta(draft.clone()));
                if sequence == 1 {
                    feeds.delta(node, session, draft.clone()).unwrap();
                }
            }
        })
        .await
        .expect("retained feed replay deadline");
        drop(updates);
        drop(transport);
        controller.close().await.unwrap();
        host.close().await.unwrap();
    }
}
