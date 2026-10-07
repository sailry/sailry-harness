use super::*;
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_protocol::{Authentication, Command, Output, ProviderId, conversation::ModelApi};
use std::time::{Duration, Instant};

mod login;

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

fn bounds(cx: &mut VisualTestContext, selector: String) -> Bounds<Pixels> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
        .unwrap()
}

fn columns(cx: &mut VisualTestContext, ids: &[usize], authorization: Option<usize>) {
    let usage = bounds(cx, format!("provider-usage-{}", ids[0]));
    for &id in ids {
        let details = bounds(cx, format!("provider-details-{id}"));
        let name = bounds(cx, format!("provider-name-{id}"));
        let row_usage = bounds(cx, format!("provider-usage-{id}"));
        assert!(details.size.width > px(0.));
        assert_eq!(name.size.width, details.size.width);
        assert!(details.right() < row_usage.left());
        assert_eq!(row_usage.size.width, px(192.));
        assert_eq!(row_usage.left(), usage.left());
        assert!(
            bounds(cx, format!("provider-kind-{id}")).right()
                < bounds(cx, format!("provider-count-{id}")).left()
        );
        for field in ["plan", "5h", "7d"] {
            let column = bounds(cx, format!("provider-{field}-{id}"));
            let first = bounds(cx, format!("provider-{field}-{}", ids[0]));
            assert_eq!(column.left(), first.left());
            assert_eq!(column.size.width, first.size.width);
        }
    }
    let first_control = bounds(cx, format!("provider-{}-control", ids[0]));
    let first_actions = bounds(cx, format!("provider-{}-actions", ids[0]));
    for &id in ids {
        let row = bounds(cx, format!("provider-{id}"));
        let control = bounds(cx, format!("provider-{id}-control"));
        let actions = bounds(cx, format!("provider-{id}-actions"));
        assert_eq!(control.size.width, px(48.));
        assert_eq!(actions.size.width, px(120.));
        assert_eq!(control.left(), first_control.left());
        assert_eq!(actions.left(), first_actions.left());
        assert!(usage.right() < control.left());
        assert!(control.right() < actions.left());
        assert_eq!(actions.right(), row.right());
        let edit = bounds(cx, format!("provider-edit-{id}"));
        let delete = bounds(cx, format!("provider-delete-{id}"));
        assert_eq!(edit.size, size(px(24.), px(24.)));
        assert_eq!(delete.size, edit.size);
        assert_eq!(edit.left(), actions.right() - px(56.));
        assert_eq!(delete.left(), actions.right() - px(24.));
        let login = cx.debug_bounds(Box::leak(format!("provider-login-{id}").into_boxed_str()));
        assert_eq!(login.is_some(), authorization == Some(id));
        if let Some(login) = login {
            assert_eq!(login.left(), actions.right() - px(88.));
            assert_eq!(login.size, edit.size);
            assert_eq!(login.center().y, edit.center().y);
        }
    }
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
    assert!(visual.debug_bounds("provider-5h-progress-0").is_none());
    assert!(visual.debug_bounds("provider-7d-progress-0").is_none());
    assert!(visual.debug_bounds("provider-manage-usage-0").is_none());
    assert!(visual.opened_url().is_none());
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    assert!(!visual.did_prompt_for_paths());
}

#[test]
fn uses_observed_remaining_and_expiry() {
    for (used, remaining_value, percentage) in [
        (0, 10000, "100"),
        (4250, 5750, "57.5"),
        (4255, 5745, "57.45"),
        (10000, 0, "0"),
        (10050, 0, "0"),
    ] {
        let window = Window {
            used_basis_points: used,
            reset_at_ms: Some(200),
        };
        assert_eq!(remaining(Some(&window), 199), Some(remaining_value));
        assert_eq!(percent(remaining_value), percentage);
        assert_eq!(remaining(Some(&window), 200), None);
        assert_eq!(remaining(Some(&window), 201), None);
    }
    assert_eq!(remaining(None, 100), None);
    assert_eq!(
        remaining(
            Some(&Window {
                used_basis_points: 4250,
                reset_at_ms: None
            }),
            u64::MAX
        ),
        Some(5750)
    );
    assert_eq!(
        remaining_label("provider_usage_5h").as_ref(),
        "5h remaining"
    );
    assert_eq!(
        remaining_label("provider_usage_7d").as_ref(),
        "7d remaining"
    );
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
        chatgpt.name = "A long ChatGPT provider name that must truncate without moving usage, switches or action buttons".into();
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
        let mut previous = px(0.);
        for width in [560., 800.] {
            visual.simulate_resize(size(px(width), px(820.)));
            draw(visual);
            columns(visual, &[0, 1], Some(chatgpt_id));
            let details = visual.debug_bounds("provider-details-0").unwrap();
            assert!(details.size.width > previous);
            previous = details.size.width;
        }
        for selector in ["provider-manage-usage-0", "provider-manage-usage-1"] {
            assert!(visual.debug_bounds(selector).is_none());
        }
        let usage = visual.debug_bounds("provider-usage-0").unwrap();
        let slot = visual.debug_bounds("provider-plan-0").unwrap();
        assert_eq!(usage.size.height, slot.size.height);
        for (id, login, edit, delete, control, content, actions) in [
            (
                0,
                "provider-login-0",
                "provider-edit-0",
                "provider-delete-0",
                "provider-0-control",
                "provider-0-content",
                "provider-0-actions",
            ),
            (
                1,
                "provider-login-1",
                "provider-edit-1",
                "provider-delete-1",
                "provider-1-control",
                "provider-1-content",
                "provider-1-actions",
            ),
        ] {
            assert_eq!(visual.debug_bounds(login).is_some(), id == chatgpt_id);
            let content = visual.debug_bounds(content).unwrap();
            let actions = visual.debug_bounds(actions).unwrap();
            let edit = visual.debug_bounds(edit).unwrap();
            let delete = visual.debug_bounds(delete).unwrap();
            let control = visual.debug_bounds(control).unwrap();
            assert!(actions.contains(&edit.center()));
            assert!(actions.contains(&delete.center()));
            assert!(edit.right() < delete.left());
            if let Some(login) = visual.debug_bounds(login) {
                assert!(actions.contains(&login.center()));
                assert!(!content.contains(&login.center()));
                assert!(control.right() < login.left());
                assert!(login.right() < edit.left());
                assert_eq!(login.center().y, edit.center().y);
                assert_eq!(login.size, edit.size);
            }
        }
        for (first, second) in [
            ("provider-edit-0", "provider-edit-1"),
            ("provider-delete-0", "provider-delete-1"),
        ] {
            assert_eq!(
                visual.debug_bounds(first).unwrap().left(),
                visual.debug_bounds(second).unwrap().left()
            );
        }
        assert!(visual.opened_url().is_none());
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
        assert!(visual.debug_bounds("provider-editor").is_some());
        for selector in [
            "provider-connect",
            "provider-authorization",
            "provider-account",
        ] {
            assert!(visual.debug_bounds(selector).is_none());
        }
        visual.update(|window, cx| window.close_dialog(cx));
        draw(visual);
        let actions = visual
            .debug_bounds(if keyed_id == 0 {
                "provider-0-actions"
            } else {
                "provider-1-actions"
            })
            .unwrap();
        tap(
            visual,
            if chatgpt_id == 0 {
                "provider-delete-0"
            } else {
                "provider-delete-1"
            },
        );
        assert_eq!(
            crate::prompts::tests::wait(visual).0,
            tr("provider_delete_title").as_ref()
        );
        crate::prompts::tests::answer(visual, "settings_delete");
        wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
        let remaining_id =
            owner.read_with(visual, |workspace, _| workspace.providers.channels[0].id);
        columns(visual, &[remaining_id], None);
        let remaining_actions = visual
            .debug_bounds(Box::leak(
                format!("provider-{remaining_id}-actions").into_boxed_str(),
            ))
            .unwrap();
        assert_eq!(remaining_actions.origin.x, actions.origin.x);
        assert_eq!(remaining_actions.size.width, actions.size.width);
        let Output::Providers(providers) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ListProviders)))
            .unwrap()
        else {
            panic!("providers expected")
        };
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].authentication, Authentication::ApiKey);
    }
    fixture.close();
}
