use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{Fault, NodeId, Request, Topic};
use std::sync::Mutex;

struct Recorded {
    inner: Arc<dyn Transport>,
    commands: Mutex<Vec<Command>>,
}

impl Transport for Recorded {
    fn target(&self) -> NodeId {
        self.inner.target()
    }

    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        self.commands.lock().unwrap().push(request.command.clone());
        self.inner.dispatch(request)
    }
}

#[gpui::test]
fn reconnects_revoked_accounts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for preset in [Preset::ChatGpt, Preset::Copilot, Preset::CopilotResponses] {
            let directory = tempfile::tempdir().unwrap();
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let server = runtime.block_on(support::server(Arc::new(AtomicU8::new(1))));
            let node = runtime
                .block_on(Node::start_with_authorization(
                    directory.path().join("node"),
                    &server.endpoint,
                ))
                .unwrap();
            let mut fixture = Fixture::with_node(remote, runtime, directory, node);
            let saved = fixture.runtime.block_on(support::connect(
                &Client::new(fixture.transport.clone()),
                support::provider(preset.authentication(), preset.api()),
            ));
            let reference = saved.credential.clone().unwrap();
            let Output::Credentials(credentials) = fixture.execute(Command::ListCredentials) else {
                panic!("credentials expected")
            };
            let revision = credentials
                .iter()
                .find(|credential| credential.id == reference.id)
                .unwrap()
                .revision;
            fixture.execute(Command::RevokeCredential {
                id: reference.id,
                expected_revision: revision,
            });
            let recorded = Arc::new(Recorded {
                inner: fixture.transport.clone(),
                commands: Mutex::new(vec![]),
            });
            fixture.transport = recorded.clone();
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let workspace = fixture.owner(window, cx);
                workspace.update(cx, |workspace, cx| {
                    workspace.providers.category = preset.category();
                    workspace.select(crate::settings::Section::Providers, cx);
                });
                owner = Some(workspace.clone());
                Root::new(cx.new(|_| Surface(workspace)), window, cx)
            });
            let owner = owner.unwrap();
            wait(visual, |cx| {
                owner.read(cx).provider_link.as_ref().unwrap().connected
                    && owner.read(cx).providers.channels.len() == 1
            });
            let editor = visual.update(|window, cx| {
                crate::settings::providers::editor::open(owner.clone(), Some(0), window, cx)
            });
            settle(visual);
            assert!(visual.debug_bounds("provider-api-key").is_none());
            editor.read_with(visual, |editor, _| {
                assert_eq!(editor.error, None);
            });
            for selector in [
                "provider-connect",
                "provider-authorization",
                "provider-account",
                "provider-login",
            ] {
                assert!(visual.debug_bounds(selector).is_none());
            }
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Unsaved account name", window, cx)
                    });
                    editor.models.clear();
                    editor.default_model = 0;
                    cx.notify();
                })
            });
            // Ordinary saves remain guarded, but OAuth failures must not ask for an API key.
            tap(visual, "provider-save");
            wait(visual, |cx| editor.read(cx).error.is_some());
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.error),
                Some("provider_account_unavailable")
            );
            crate::feedback::tests::shown(visual);
            assert_eq!(
                visual.update(crate::feedback::tests::summary),
                tr("provider_account_unavailable")
            );
            visual.update(|window, cx| window.clear_notifications(cx));
            editor.read_with(visual, |editor, cx| {
                assert!(!editor.closed && !editor.pending);
                assert!(editor.models.is_empty());
                assert_eq!(
                    editor.name.read(cx).value().as_ref(),
                    "Unsaved account name"
                );
            });
            tap(visual, "provider-cancel");
            assert!(editor.read_with(visual, |editor, _| editor.closed));
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            settle(visual);
            let before = recorded.commands.lock().unwrap().len();
            tap(visual, "provider-login-0");
            if preset == Preset::ChatGpt {
                let deadline = Instant::now() + Duration::from_secs(10);
                while visual.debug_bounds("provider-login-waiting").is_none() {
                    settle(visual);
                    assert!(Instant::now() < deadline, "browser readiness deadline");
                }
                let authorization = visual.opened_url().unwrap();
                fixture.runtime.block_on(async {
                    reqwest::get(support::callback(&authorization).expose())
                        .await
                        .unwrap()
                        .error_for_status()
                        .unwrap();
                });
            }
            wait(visual, |cx| {
                owner.read(cx).providers.channels[0].credential_configured
                    && owner.read(cx).provider_link.as_ref().unwrap().providers[&0].revision
                        > saved.revision
            });
            settle(visual);
            assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
            let commands = recorded.commands.lock().unwrap();
            assert!(commands[before..].iter().any(|command| matches!(command,
                Command::BeginProviderLogin { provider, .. } if *provider == saved.id
            )));
            assert!(!commands[before..].iter().any(|command| matches!(
                command,
                Command::SaveProvider { .. } | Command::ReadProviderKey { .. }
            )));
            drop(commands);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].name, saved.name);
            assert_eq!(providers[0].credential, Some(reference.clone()));
            let Output::Credentials(credentials) = fixture.execute(Command::ListCredentials) else {
                panic!("credentials expected")
            };
            assert_eq!(credentials.len(), 1);
            assert_eq!(credentials[0].id, reference.id);
            assert_eq!(credentials[0].revision, revision + 2);
            assert!(!credentials[0].revoked);
            let editor = visual.update(|window, cx| {
                crate::settings::providers::editor::open(owner.clone(), Some(0), window, cx)
            });
            settle(visual);
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert_eq!(editor.name.read(cx).value().as_ref(), saved.name);
                assert_eq!(
                    editor.provider.as_ref().unwrap().credential,
                    Some(reference.clone())
                );
            });
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Saved account name", window, cx)
                    });
                    editor.models.clear();
                    editor.default_model = 0;
                    cx.notify();
                })
            });
            tap(visual, "provider-discover");
            wait(visual, |cx| {
                editor.read(cx).probe.is_none() && !editor.read(cx).models.is_empty()
            });
            tap(visual, "provider-save");
            wait(visual, |cx| editor.read(cx).closed);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].name, "Saved account name");
            assert_eq!(providers[0].credential, Some(reference));
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
