use super::*;
use crate::{
    browser::Browser, conversation::live::View as Chat, plugins::Panel,
    resources::launcher::Destination,
};

fn draft(
    shell: &Entity<Shell>,
    cx: &mut VisualTestContext,
    fixture: &Fixture,
    index: usize,
) -> Entity<Chat> {
    open(shell, cx, fixture, index, &fixture.sessions[index]);
    let binding = shell.read_with(cx, |shell, cx| {
        shell.current_chat().unwrap().read(cx).binding()
    });
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.new_live_conversation(window, cx));
    });
    let draft = shell.read_with(cx, |shell, _| shell.current_chat().unwrap().clone());
    cx.update(|window, cx| {
        draft.update(cx, |draft, cx| draft.retarget(binding, window, cx));
        shell.update(cx, |shell, cx| shell.restore_session_scope(cx));
    });
    wait(cx, |cx| {
        draft.read(cx).connected()
            && draft.read(cx).binding().client.target() == fixture.nodes[index].id()
    });
    assert!(draft.read_with(cx, |draft, _| draft.session().is_none()));
    draft
}

fn page(shell: &Entity<Shell>, cx: &mut VisualTestContext) -> Entity<Panel> {
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.open_destination(Destination::Browser, window, cx)
        });
    });
    let panel = shell.read_with(cx, |shell, _| {
        let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
            panic!("browser plugin page expected")
        };
        panel.clone()
    });
    wait(cx, |cx| {
        panel.read(cx).resource_active() && panel.read(cx).matches_source(cx)
    });
    panel
}

fn tabs(browser: &Entity<Browser>, cx: &mut VisualTestContext) -> serde_json::Value {
    browser.read_with(cx, |browser, cx| browser.snapshot(cx)["tabs"].clone())
}

#[gpui::test]
fn promotes_scoped_drafts_after_first_send(cx: &mut TestAppContext) {
    let fixture = Fixture::new();
    for node in &fixture.nodes {
        let client = Client::new(node.local());
        let Output::Plugin(info) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadPlugin {
                name: "browser".into(),
            })))
            .unwrap()
        else {
            panic!("browser package expected")
        };
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::SetPluginEnabled {
                name: info.summary.name,
                expected_revision: info.summary.revision,
                enabled: true,
            })))
            .unwrap();
    }
    let (shell, visual) = mount(cx, &fixture);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));

    // Both drafts exist before either first send, so the shared Draft tab key
    // cannot let the remote controller inherit the local controller's tabs.
    let mut browsers = Vec::new();
    for index in 0..2 {
        draft(&shell, visual, &fixture, index);
        let panel = page(&shell, visual);
        let browser = panel.read_with(visual, |panel, _| panel.browser_view().unwrap());
        assert_eq!(tabs(&browser, visual).as_array().unwrap().len(), 1);
        if let Some(previous) = browsers.first() {
            assert_ne!(&browser, previous);
        }
        click(&shell, visual, "browser-new-tab".into());
        wait(visual, |cx| {
            browser.read(cx).snapshot(cx)["tabs"]
                .as_array()
                .unwrap()
                .len()
                == 2
        });
        browsers.push(browser);
    }

    for (index, browser) in browsers.iter().enumerate() {
        let draft = draft(&shell, visual, &fixture, index);
        let before = page(&shell, visual);
        assert_eq!(
            before.read_with(visual, |panel, _| panel.browser_view().unwrap()),
            *browser
        );
        let expected = tabs(browser, visual);
        assert_eq!(expected.as_array().unwrap().len(), 2);
        click(&shell, visual, "live-chat-input".into());
        visual.simulate_input("Create a conversation with the current browser tabs");
        click(&shell, visual, "live-chat-send".into());
        wait(visual, |cx| {
            let Some(session) = draft.read(cx).session() else {
                return false;
            };
            let shell = shell.read(cx);
            shell.session_scope.active == Key::Session(fixture.nodes[index].id(), session)
                && matches!(&shell.side_resource, Some(SideResource::Plugin(panel))
                    if *panel != before && panel.read(cx).resource_active()
                        && panel.read(cx).matches_source(cx))
        });
        shell.read_with(visual, |shell, cx| {
            let Some(SideResource::Plugin(panel)) = &shell.side_resource else {
                panic!("promoted browser page expected")
            };
            assert_eq!(panel.read(cx).source.as_ref(), Some(&draft));
            assert_eq!(panel.read(cx).browser_view().as_ref(), Some(browser));
            assert!(draft.read(cx).session().is_some());
            assert!(panel.read(cx).matches_source(cx));
            assert_eq!(browser.read(cx).snapshot(cx)["selected"], 1);
        });
        assert_eq!(tabs(browser, visual), expected);
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}
