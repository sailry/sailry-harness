use super::*;
use crate::{backend::Services, shell::Shell};
use sailry_node_runtime::Node;
use sailry_protocol::Session;

pub(super) fn desktop(fixture: &Fixture) -> Node {
    let desktop = fixture
        .runtime
        .block_on(Node::start(fixture.directory.path().join("desktop")))
        .unwrap();
    fixture
        .runtime
        .block_on(
            desktop
                .link()
                .pair(fixture.node.link().invite().unwrap().ticket()),
        )
        .unwrap();
    desktop
}

pub(super) fn open<'a>(
    fixture: &Fixture,
    desktop: &Node,
    remote: bool,
    session: Session,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, Entity<View>, &'a mut VisualTestContext) {
    let local = if remote { desktop } else { &fixture.node };
    cx.update(|cx| {
        crate::shell::init(cx);
        cx.set_global(Services {
            runtime: fixture.runtime.clone(),
            local: local.local(),
            link: local.link(),
            relay_enabled: false,
        });
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .hosts
            .contains_key(&desktop.id())
            && shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&fixture.node.id())
    });
    visual.update(|_, cx| {
        shell.update(cx, |shell, cx| {
            shell.live.as_mut().unwrap().select(fixture.node.id(), cx)
        })
    });
    wait(visual, |cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.sessions.iter().any(|item| item.id == session.id))
    });
    visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.reveal_session(session.clone(), window, cx)
        })
    });
    let view = shell.read_with(visual, |shell, _| shell.current_chat().unwrap().clone());
    wait(visual, |cx| view.read(cx).connected());
    (shell, view, visual)
}

pub(super) fn finished(fixture: &Fixture) -> sailry_protocol::conversation::History {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let Output::Conversation(history) = fixture.execute(Command::ReadConversation {
            session: fixture.session.id,
            before: None,
            limit: 100,
        }) else {
            panic!("conversation expected")
        };
        if !history.page.runs.is_empty()
            && history
                .page
                .runs
                .iter()
                .all(|run| run.status == Status::Completed)
        {
            return history;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture completion deadline"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
