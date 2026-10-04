use super::*;
use sailry_protocol::RequestId;
use std::sync::atomic::Ordering;

#[path = "../../../../../../crates/node-runtime/tests/support/trash.rs"]
mod trash;

#[gpui::test]
#[ignore = "moves uniquely named fixtures into the system Trash and recovers them"]
fn restores_absence_once(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let name = format!("sailry-checkpoint-ui-{}-资料.txt", RequestId::new());
        let fixture = Fixture::with_new(remote, 1, &name);
        let entry = trash::Entry::new(&fixture.chat.directory.path().join("project"), name.clone());
        assert_eq!(entry.name, name);
        let mut transport = None;
        let (shell, visual) = fixture.mount_with(cx, |inner| {
            let next = lifecycle::observed(inner);
            transport = Some(next.clone());
            next
        });
        let transport = transport.unwrap();
        fixture::open_path(visual, &shell, &name);
        let state = restore(visual, &fixture, &shell);
        start(visual, &state, Some("not-a-changed-file"));
        transport.drop_next.store(true, Ordering::SeqCst);
        start(visual, &state, Some(&name));
        assert_eq!(
            state.read_with(visual, |state, _| state.issue),
            Some("checkpoint_unknown")
        );
        assert!(!entry.source.exists());
        let request = transport.requests.lock().unwrap().last().unwrap().clone();
        std::fs::write(&entry.source, "Replacement survives retry").unwrap();
        start(visual, &state, Some(&name));
        assert!(state.read_with(visual, |state, _| state.restored(Some(&name))));
        assert_eq!(transport.requests.lock().unwrap().last().unwrap(), &request);
        assert!(visual.update(|_, cx| {
            fixture::documents(&shell, cx)
                .read(cx)
                .editor(&name)
                .is_none()
        }));
        assert_eq!(
            std::fs::read_to_string(&entry.source).unwrap(),
            "Replacement survives retry"
        );
        assert_eq!(
            std::fs::read_to_string(entry.recover()).unwrap(),
            "Created 中文 🙂"
        );
        start(visual, &state, None);
        assert!(state.read_with(visual, |state, _| state.restored(None)));
        assert_eq!(
            std::fs::read_to_string(fixture.chat.directory.path().join("project/file-0.txt"))
                .unwrap(),
            before(0)
        );
        fixture.close();
    }
}
