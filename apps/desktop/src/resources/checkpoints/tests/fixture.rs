use super::*;
use crate::{backend::Services, conversation::live::ChatFixture};
use gpui_kit::component::{Root, WindowExt, dialog, input::EditorState};
use sailry_link::Transport;
use sailry_node_runtime::Node;
use sailry_protocol::{
    Permission,
    conversation::{Page, Status},
};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(super) struct Fixture {
    pub chat: ChatFixture,
    desktop: Option<Node>,
    pub turn: TurnId,
    pub original: Page,
}

impl Fixture {
    pub fn new(remote: bool, count: usize) -> Self {
        Self::with_new(remote, count, "new.txt")
    }

    pub fn with_new(remote: bool, count: usize, created: &str) -> Self {
        Self::writes(remote, count, created, false)
    }

    pub fn writes(remote: bool, count: usize, created: &str, repeated: bool) -> Self {
        let changes: Vec<_> = (0..count)
            .map(|index| {
                let path = format!("file-{index}.txt");
                (path, before(index), after(index))
            })
            .collect();
        let mut calls: Vec<_> = changes.iter().map(|(path, before, after)| (format!("plugin_{}_write_file", &blake3::hash(b"files").to_hex()[..16]), json!({"path": path, "text": after, "expected_revision": blake3::hash(before.as_bytes()).to_hex().to_string()}))).collect();
        calls.push((
            format!(
                "plugin_{}_write_file",
                &blake3::hash(b"files").to_hex()[..16]
            ),
            json!({"path": created, "text": "Created 中文 🙂", "expected_revision": null}),
        ));
        if repeated {
            calls.push((format!("plugin_{}_write_file", &blake3::hash(b"files").to_hex()[..16]), json!({"path": "file-0.txt", "text": "Second write", "expected_revision": blake3::hash(after(0).as_bytes()).to_hex().to_string()})));
        }
        let mut chat = ChatFixture::with_tools(false, calls);
        for (path, before, _) in changes {
            std::fs::write(chat.directory.path().join("project").join(path), before).unwrap();
        }
        let mut config = chat.session.config.clone();
        config.permission = Permission::Full;
        let Output::Session(session) = chat.execute(Command::SetSessionConfig {
            session: chat.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        chat.session = session;
        let Output::QueuedTurn(turn) = chat.execute(Command::SubmitTurn {
            session: chat.session.id,
            expected_revision: 2,
            message: "Change the fixture files".into(),
        }) else {
            panic!("turn expected")
        };
        let original = chat.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    let page = chat
                        .binding
                        .client
                        .read_conversation(chat.session.id, None, 100)
                        .await
                        .unwrap()
                        .page;
                    if page.runs[0].status == Status::Completed {
                        break page;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap()
        });
        let Output::FileCheckpoints(page) = chat.execute(Command::ListFileCheckpoints {
            session: chat.session.id,
            turn: turn.id,
            before: None,
            limit: 100,
        }) else {
            panic!("checkpoints expected")
        };
        assert_eq!(page.files.len(), count + 1 + usize::from(repeated));
        let desktop = remote.then(|| {
            let node = chat
                .runtime
                .block_on(Node::start(chat.directory.path().join("desktop")))
                .unwrap();
            chat.runtime
                .block_on(
                    node.link()
                        .pair(chat.node.link().invite().unwrap().ticket()),
                )
                .unwrap();
            node
        });
        Self {
            chat,
            desktop,
            turn: turn.id,
            original,
        }
    }

    pub fn mount<'a>(
        &self,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Shell>, &'a mut VisualTestContext) {
        self.mount_with(cx, |transport| transport)
    }

    pub fn mount_with<'a>(
        &self,
        cx: &'a mut TestAppContext,
        transport: impl FnOnce(Arc<dyn Transport>) -> Arc<dyn Transport>,
    ) -> (Entity<Shell>, &'a mut VisualTestContext) {
        let local = self.desktop.as_ref().unwrap_or(&self.chat.node);
        cx.update(|cx| {
            cx.set_global(Services {
                runtime: self.chat.runtime.clone(),
                local: local.local(),
                link: local.link(),
                relay_enabled: false,
            })
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
                .contains_key(&self.chat.node.id())
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(self.chat.node.id(), cx)
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
                .is_some_and(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .any(|session| session.id == self.chat.session.id)
                })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let live = shell.live.as_mut().unwrap();
                live.transport = transport(live.transport.clone());
                shell.live.as_mut().unwrap().project = self.chat.session.project;
                shell.new_live_conversation(window, cx);
            })
        });
        tap(visual, &format!("live-session-{}", self.chat.session.id));
        wait(visual, |cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                chat.read(cx).session() == Some(self.chat.session.id) && chat.read(cx).connected()
            })
        });
        (shell, visual)
    }

    pub fn close(self) {
        if let Some(desktop) = self.desktop {
            self.chat.runtime.block_on(desktop.shutdown()).unwrap();
        }
        self.chat.close();
    }
}

pub(super) fn before(index: usize) -> String {
    format!("Before {index} 中文 🙂\n").repeat(20)
}
pub(super) fn after(index: usize) -> String {
    format!("After {index} 中文 🙂\n").repeat(30)
}

pub(super) fn init(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
    });
}

pub(super) fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            let _ = window.draw(cx);
            predicate(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "checkpoint UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn tap(cx: &mut VisualTestContext, selector: &str) {
    if cx.update(|window, cx| window.has_active_dialog(cx)) {
        std::thread::sleep(*dialog::ANIMATION_DURATION);
    }
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let selector = Box::leak(selector.to_owned().into_boxed_str());
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_mouse_move(bounds.center(), None, Modifiers::default());
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

pub(super) fn copied(cx: &mut VisualTestContext) -> String {
    cx.simulate_keystrokes("secondary-a secondary-c");
    cx.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap())
}

pub(super) fn restore(
    cx: &mut VisualTestContext,
    fixture: &Fixture,
    shell: &Entity<Shell>,
) -> Entity<Restore> {
    cx.update(|_, cx| {
        let source = shell.read(cx).current_chat().unwrap().clone();
        let binding = source.read(cx).binding();
        cx.new(|_| {
            Restore::new(
                shell.downgrade(),
                source.downgrade(),
                binding,
                fixture.chat.session.id,
                fixture.turn,
            )
        })
    })
}

pub(super) fn start(cx: &mut VisualTestContext, state: &Entity<Restore>, path: Option<&str>) {
    cx.update(|window, cx| {
        state.update(cx, |state, cx| {
            state.start(path.map(str::to_owned), window, cx)
        })
    });
    wait(cx, |cx| !state.read(cx).pending);
}

pub(super) fn documents(
    shell: &Entity<Shell>,
    cx: &App,
) -> Entity<crate::plugins::documents::Controller> {
    let Some(crate::resources::SideResource::Plugin(panel)) = &shell.read(cx).side_resource else {
        panic!("document panel expected");
    };
    panel
        .read(cx)
        .documents
        .clone()
        .expect("document controller")
}

pub(super) fn open_path(
    cx: &mut VisualTestContext,
    shell: &Entity<Shell>,
    path: &str,
) -> Entity<EditorState> {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            let chat = shell.current_chat().unwrap().read(cx);
            let binding = chat.binding();
            shell.open_documents(
                (binding.client.target(), binding.worktree.unwrap()),
                Some((path.into(), None)),
                window,
                cx,
            );
        })
    });
    wait(cx, |cx| match &shell.read(cx).side_resource {
        Some(crate::resources::SideResource::Plugin(panel)) => panel
            .read(cx)
            .documents
            .as_ref()
            .is_some_and(|documents| documents.read(cx).editor(path).is_some()),
        _ => false,
    });
    cx.update(|_, cx| documents(shell, cx).read(cx).editor(path).unwrap())
}

pub(super) fn open_file(cx: &mut VisualTestContext, shell: &Entity<Shell>) -> Entity<EditorState> {
    open_path(cx, shell, "file-0.txt")
}
