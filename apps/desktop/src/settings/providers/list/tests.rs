use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_protocol::{Authentication, Command, Output, ProviderId, conversation::ModelApi};
use std::time::{Duration, Instant};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "provider list update deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn tap(cx: &mut VisualTestContext, selector: &'static str) {
    draw(cx);
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

#[gpui::test]
fn keeps_preview_passive(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.providers.channels[0].preset = Preset::ChatGpt;
            workspace.select(crate::settings::Section::Providers, cx);
        });
        Root::new(workspace, window, cx)
    });
    draw(visual);
    assert!(visual.debug_bounds("provider-login-0").is_none());
    for field in [
        "provider-plan-value-0",
        "provider-5h-value-0",
        "provider-7d-value-0",
    ] {
        assert!(visual.debug_bounds(field).is_some());
    }
    assert_eq!(tr("provider_usage_unknown").as_ref(), "—");
    tap(visual, "provider-manage-usage-0");
    assert!(visual.opened_url().is_none());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!visual.did_prompt_for_paths());
}

#[gpui::test]
fn aligns_usage_and_keeps_actions(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let fixture = super::super::fixture::Fixture::new();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let mut chatgpt = fixture.providers[index].clone();
        let revision = chatgpt.revision;
        chatgpt.name = "ChatGPT fixture".into();
        chatgpt.api = ModelApi::Responses;
        chatgpt.authentication = Authentication::ChatGpt;
        chatgpt.endpoint = "https://api.openai.com/v1".into();
        let Output::Provider(chatgpt) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::PutProvider {
                provider: chatgpt,
                expected_revision: revision,
            })))
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut keyed = chatgpt.clone();
        keyed.id = ProviderId::new();
        keyed.revision = 0;
        keyed.name = "API-key fixture".into();
        keyed.authentication = Authentication::ApiKey;
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::PutProvider {
                provider: keyed,
                expected_revision: 0,
            })))
            .unwrap();
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport.clone(),
                    fixture.runtime.clone(),
                    "Provider Node".into(),
                    cx,
                );
                workspace.select(crate::settings::Section::Providers, cx);
            });
            owner = Some(workspace.clone());
            Root::new(workspace, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| owner.read(cx).providers.channels.len() == 2);
        let (chatgpt_id, keyed_id) = owner.read_with(visual, |workspace, _| {
            let channels = &workspace.providers.channels;
            (
                channels
                    .iter()
                    .find(|channel| channel.preset == Preset::ChatGpt)
                    .unwrap()
                    .id,
                channels
                    .iter()
                    .find(|channel| channel.preset == Preset::OpenAi)
                    .unwrap()
                    .id,
            )
        });
        assert_eq!([chatgpt_id.min(keyed_id), chatgpt_id.max(keyed_id)], [0, 1]);
        for (first, second) in [
            ("provider-usage-0", "provider-usage-1"),
            ("provider-plan-0", "provider-plan-1"),
            ("provider-5h-0", "provider-5h-1"),
            ("provider-7d-0", "provider-7d-1"),
        ] {
            let first = visual.debug_bounds(first).unwrap();
            let second = visual.debug_bounds(second).unwrap();
            assert_eq!(first.left(), second.left());
            assert_eq!(first.size.width, second.size.width);
        }
        assert!(
            visual
                .debug_bounds(if keyed_id == 0 {
                    "provider-manage-usage-0"
                } else {
                    "provider-manage-usage-1"
                })
                .is_none()
        );
        for (login, edit, delete, control) in [
            (
                "provider-login-0",
                "provider-edit-0",
                "provider-delete-0",
                "provider-0-control",
            ),
            (
                "provider-login-1",
                "provider-edit-1",
                "provider-delete-1",
                "provider-1-control",
            ),
        ] {
            assert!(visual.debug_bounds(login).is_none());
            assert!(visual.debug_bounds(edit).is_some());
            assert!(visual.debug_bounds(delete).is_some());
            assert!(visual.debug_bounds(control).is_some());
        }
        tap(
            visual,
            if chatgpt_id == 0 {
                "provider-manage-usage-0"
            } else {
                "provider-manage-usage-1"
            },
        );
        assert_eq!(visual.opened_url().as_deref(), Some(USAGE_URL));
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        tap(
            visual,
            if chatgpt_id == 0 {
                "provider-0-control"
            } else {
                "provider-1-control"
            },
        );
        wait(visual, |cx| {
            owner
                .read(cx)
                .providers
                .channels
                .iter()
                .any(|channel| channel.id == chatgpt_id && !channel.enabled)
        });
        let Output::Providers(providers) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ListProviders)))
            .unwrap()
        else {
            panic!("providers expected")
        };
        assert!(
            !providers
                .iter()
                .find(|provider| provider.id == chatgpt.id)
                .unwrap()
                .enabled
        );
        tap(
            visual,
            if chatgpt_id == 0 {
                "provider-edit-0"
            } else {
                "provider-edit-1"
            },
        );
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(visual.debug_bounds("provider-connect").is_some());
        visual.update(|window, cx| window.close_dialog(cx));
    }
    fixture.close();
}
