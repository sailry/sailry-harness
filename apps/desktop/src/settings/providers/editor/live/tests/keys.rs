use super::*;
use crate::settings::providers::editor::open;
use sailry_protocol::Secret;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    let duration = cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        cx.theme().motion_tokens().duration_normal
    });
    cx.executor().advance_clock(duration);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn reopens_replaces_and_clears(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let fixture = crate::settings::providers::fixture::Fixture::new();
    for (index, transport) in fixture.transports.iter().enumerate() {
        let client = Client::new(transport.clone());
        let original = fixture.providers[index].clone();
        let Output::Provider(mut saved) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::SaveProvider {
                expected_revision: original.revision,
                provider: original,
                secret: Some(Secret::new("synthetic-original-key".into())),
            })))
            .unwrap()
        else {
            panic!("provider expected")
        };
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    transport.clone(),
                    fixture.runtime.clone(),
                    "Credential Node".into(),
                    cx,
                );
            });
            owner = Some(workspace.clone());
            let surface = cx.new(|_| Surface(workspace));
            Root::new(surface, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().is_some_and(|live| {
                live.connected && live.providers.values().any(|value| value == &saved)
            })
        });
        let id = owner.read_with(visual, |owner, _| owner.providers.channels[0].id);

        for (replacement, expected) in [
            (None, "synthetic-original-key"),
            (Some("synthetic-replacement-key"), "synthetic-original-key"),
            (Some(""), "synthetic-replacement-key"),
        ] {
            let editor = visual.update(|window, cx| open(owner.clone(), Some(id), window, cx));
            wait(visual, |cx| !editor.read(cx).pending);
            draw(visual);
            assert!(visual.debug_bounds("provider-api-key").is_some());
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                let input = editor.credential.read(cx);
                assert_eq!(input.value().as_str(), expected);
                assert!(!input.presentation().is_masked());
                assert_eq!(
                    input.presentation().placeholder().as_ref(),
                    tr("provider_key_hint").as_ref()
                );
            });
            if let Some(replacement) = replacement {
                visual.update(|window, cx| {
                    let credential = editor.read(cx).credential.clone();
                    credential.update(cx, |input, cx| {
                        input.focus(window, cx);
                    });
                });
                visual.simulate_keystrokes("secondary-a backspace");
                if !replacement.is_empty() {
                    visual.simulate_input(replacement);
                }
                editor.read_with(visual, |editor, cx| {
                    assert_eq!(editor.credential.read(cx).value().as_str(), replacement);
                });
            } else {
                visual.update(|window, cx| {
                    let name = editor.read(cx).name.clone();
                    name.update(cx, |input, cx| {
                        input.set_value("Renamed provider", window, cx);
                    });
                });
            }
            draw(visual);
            let save = visual.debug_bounds("provider-save").unwrap();
            visual.simulate_click(save.center(), Modifiers::default());
            wait(visual, |cx| {
                !editor.read(cx).pending
                    && owner.read(cx).provider_link.as_ref().unwrap().providers[&id].revision
                        == saved.revision + 1
            });
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert!(editor.closed);
                assert!(editor.credential.read(cx).value().is_empty());
                assert!(editor.original_key.is_none());
            });
            let Output::Providers(providers) = fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::ListProviders)))
                .unwrap()
            else {
                panic!("providers expected")
            };
            let next = providers
                .into_iter()
                .find(|value| value.id == saved.id)
                .unwrap();
            match replacement {
                None => assert_eq!(next.credential, saved.credential),
                Some("") => assert!(next.credential.is_none()),
                Some(_) => {
                    assert!(next.credential.is_some());
                    assert_ne!(next.credential, saved.credential);
                }
            }
            let Output::ProviderKey(key) = fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::ReadProviderKey {
                    provider: next.id,
                    expected_revision: next.revision,
                })))
                .unwrap()
            else {
                panic!("provider key expected")
            };
            assert_eq!(
                key.as_ref().map(Secret::expose),
                replacement
                    .or(Some(expected))
                    .filter(|value| !value.is_empty())
            );
            saved = next;
            draw(visual);
        }

        let editor = visual.update(|window, cx| open(owner.clone(), Some(id), window, cx));
        wait(visual, |cx| !editor.read(cx).pending);
        editor.read_with(visual, |editor, cx| {
            assert!(editor.credential.read(cx).value().is_empty());
            assert!(editor.original_key.is_none());
        });
        draw(visual);
        let cancel = visual.debug_bounds("provider-cancel").unwrap();
        visual.simulate_click(cancel.center(), Modifiers::default());
        assert!(editor.read_with(visual, |editor, _| editor.closed));
        visual.update(|window, _| window.remove_window());
    }
    fixture.close();
}
