use super::*;

fn package(
    fixture: &Fixture,
    control: bool,
    props: serde_json::Value,
) -> sailry_protocol::plugin::Info {
    let current = super::package(fixture, control);
    let source = format!(
        r#"
import {{View,div}} from 'gpui-kit';
import {{Button}} from 'gpui-component';
import {{Conversation}} from 'sailry';
import {{Bounds}} from 'sailry/test';
export default class Detail extends View {{
  init() {{this.active=true;this.props={props};}}
  render() {{return div().size_full()
    .child(new Button('detail-hide').relative().label('Close').child(Bounds.new('detail-hide'))
      .on_click((_event,cx)=>{{this.active=false;cx.notify();}}))
    .child(Conversation.new('detail',{{...this.props,active:this.active}}));}}
}}
"#
    );
    std::fs::write(
        fixture
            .directory
            .path()
            .join("project/package/dev.sailry.platform/desktop/chat.js"),
        source,
    )
    .unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "package".into(),
        name: "task-notes".into(),
        expected_revision: current.summary.revision,
        expected_digest: None,
    }) else {
        panic!("plugin expected");
    };
    assert!(info.issues.is_empty(), "{:?}", info.issues);
    info
}

fn chat(panel: &Entity<Panel>, cx: &App) -> Option<Entity<crate::conversation::live::View>> {
    panel.read(cx).mounted.as_ref()?.conversations.first()
}

#[gpui::test]
fn selects_existing_workspace_and_releases(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let directory = fixture.directory.path().join("other-project");
        std::fs::create_dir(&directory).unwrap();
        let Output::Project(project) = fixture.execute(Command::RegisterProject {
            name: "Another project".into(),
            path: directory.to_str().unwrap().into(),
        }) else {
            panic!("project expected");
        };
        let Output::Session(session) = fixture.execute(Command::CreateSession {
            project: Some(project.id),
            worktree: None,
            config: Some(fixture.session.config.clone()),
        }) else {
            panic!("session expected");
        };
        let package = package(&fixture, true, serde_json::json!({"session":session.id}));
        let (panel, visual) = super::assistants::mount(&fixture, cx);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| {
                panel.open(package.summary.reference(), window, cx)
            })
        });
        wait(visual, |cx| {
            chat(&panel, cx).is_some_and(|chat| chat.read(cx).connected())
        });
        let embedded = panel.read_with(visual, |_, cx| chat(&panel, cx).unwrap());
        embedded.read_with(visual, |view, _| {
            assert_eq!(view.session(), Some(session.id));
            assert_eq!(view.binding().client.target(), fixture.node.id());
            assert_eq!(view.binding().project, Some(project.id));
            assert_eq!(view.binding().worktree, Some(session.worktree));
            assert_eq!(view.binding().project_name, "Another project");
        });
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected");
        };
        assert_eq!(
            snapshot.sessions.len(),
            2,
            "embedding must not create a session"
        );
        let weak = embedded.downgrade();
        drop(embedded);
        click(visual, "detail-hide");
        wait(visual, |cx| {
            chat(&panel, cx).is_none() && weak.upgrade().is_none()
        });
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}

#[gpui::test]
fn rejects_missing_sessions_and_conflicting_props(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        for kind in ["missing", "conflicting", "undeclared"] {
            let fixture = Fixture::new(remote);
            let props = match kind {
                "missing" => serde_json::json!({"session":sailry_protocol::SessionId::new()}),
                "conflicting" => {
                    serde_json::json!({"session":fixture.session.id,"assistant":"notes"})
                }
                _ => serde_json::json!({"session":fixture.session.id}),
            };
            let package = package(&fixture, kind != "undeclared", props);
            let (panel, visual) = super::assistants::mount(&fixture, cx);
            visual.update(|window, cx| {
                panel.update(cx, |panel, cx| {
                    panel.open(package.summary.reference(), window, cx)
                })
            });
            wait(visual, |cx| {
                panel.read(cx).mounted.is_some() && !snapshot(&panel, cx).is_empty()
            });
            assert!(
                visual
                    .debug_bounds("plugin-conversation-unavailable")
                    .is_some()
            );
            assert!(panel.read_with(visual, |_, cx| chat(&panel, cx).is_none()));
            visual.update(|window, _| window.remove_window());
            drop(panel);
            fixture.close();
        }
    }
}
