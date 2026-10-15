use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{Fault, NodeId, Request, Topic};
use std::sync::{Arc, Mutex};

struct Recorded {
    inner: Arc<dyn Transport>,
    requests: Mutex<Vec<Request>>,
}

impl Recorded {
    fn new(inner: Arc<dyn Transport>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            requests: Mutex::new(Vec::new()),
        })
    }
}

impl Transport for Recorded {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.requests.lock().unwrap().push(request.clone());
        self.inner.dispatch(request)
    }
}

#[gpui::test]
fn legacy_layout_does_not_restore_views(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    let node = fixture.nodes[0].id();
    let targets: Vec<_> = (0..256)
        .map(|_| crate::panes::Target::Session(node, sailry_protocol::SessionId::new()))
        .collect();
    let groups: Vec<_> = targets
        .iter()
        .map(|target| {
            serde_json::json!({
                "panel_name": "sailry-workspace-pane",
                "children": [],
                "info": { "panel": target }
            })
        })
        .collect();
    let mut preferences = serde_json::to_value(crate::preferences::Data::default()).unwrap();
    preferences["workspaces"] = serde_json::json!({
        "active": targets.first(),
        "groups": groups,
        "parents": []
    });
    std::fs::write(&path, serde_json::to_vec(&preferences).unwrap()).unwrap();

    let mut services = fixture.services();
    let transport = Recorded::new(services.local.clone());
    services.local = transport.clone();
    cx.update(|cx| {
        cx.set_global(crate::preferences::Preferences::open(path));
    });
    let (shell, visual) = mount_with_services(cx, services);
    shell.read_with(visual, |shell, _| {
        assert!(shell.chats.views.is_empty());
        assert!(shell.session_scope.open.is_empty());
    });
    let requests = transport.requests.lock().unwrap();
    assert!(!requests.iter().any(|request| {
        matches!(
            &request.command,
            Command::Snapshot | Command::ReadTurn { .. }
        )
    }));
    drop(requests);

    let session = fixture.sessions[0].clone();
    open(&shell, visual, &fixture, 0, &session);
    let first = shell.read_with(visual, |shell, _| {
        assert_eq!(shell.chats.views.len(), 1);
        shell.chats.views[&(fixture.nodes[0].id(), session.id)].entity_id()
    });
    open(&shell, visual, &fixture, 0, &session);
    shell.read_with(visual, |shell, _| {
        assert_eq!(shell.chats.views.len(), 1);
        assert_eq!(
            shell.chats.views[&(fixture.nodes[0].id(), session.id)].entity_id(),
            first
        );
    });

    let target = crate::panes::Target::Session(fixture.nodes[0].id(), session.id);
    visual.update(|window, cx| shell.update(cx, |shell, cx| shell.remove_pane(target, window, cx)));
    wait(visual, |_| {
        transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| matches!(request.command, Command::StopCommands { .. }))
    });
    assert!(
        !transport
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| matches!(request.command, Command::StopTurn { .. }))
    );

    visual.update(|window, _| window.remove_window());
    fixture.close();
}
