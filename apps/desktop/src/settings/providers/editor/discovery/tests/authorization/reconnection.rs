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
                assert!(editor.authorization.is_none());
            });
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
            let before = recorded.commands.lock().unwrap().len();
            tap(visual, "provider-connect");
            wait(visual, |cx| editor.read(cx).authorization.is_some());
            if preset == Preset::ChatGpt {
                wait(visual, |cx| {
                    editor.read(cx).authorization.as_ref().is_some_and(|login| {
                        matches!(login.read(cx).state(), Some(login::State::Browser { .. }))
                    })
                });
                let authorization = editor.read_with(visual, |editor, cx| {
                    let Some(login::State::Browser {
                        authorization_url, ..
                    }) = editor.authorization.as_ref().unwrap().read(cx).state()
                    else {
                        panic!("browser authorization expected")
                    };
                    authorization_url.clone()
                });
                fixture.runtime.block_on(async {
                    reqwest::get(support::callback(&authorization).expose())
                        .await
                        .unwrap()
                        .error_for_status()
                        .unwrap();
                });
            }
            wait(visual, |cx| {
                editor.read(cx).authorization.as_ref().is_some_and(|login| {
                    matches!(login.read(cx).state(), Some(login::State::Connected))
                }) && !editor.read(cx).pending
                    && editor
                        .read(cx)
                        .provider
                        .as_ref()
                        .is_some_and(|provider| provider.revision > saved.revision)
            });
            editor.read_with(visual, |editor, cx| {
                assert_eq!(editor.error, None);
                assert!(!editor.closed);
                assert!(editor.models.is_empty());
                assert_eq!(
                    editor.name.read(cx).value().as_ref(),
                    "Unsaved account name"
                );
                assert_eq!(
                    editor.provider.as_ref().unwrap().credential,
                    Some(reference.clone())
                );
            });
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
            tap(visual, "provider-discover");
            wait(visual, |cx| {
                editor.read(cx).probe.is_none() && !editor.read(cx).models.is_empty()
            });
            tap(visual, "provider-save");
            wait(visual, |cx| editor.read(cx).closed);
            let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
                panic!("providers expected")
            };
            assert_eq!(providers[0].name, "Unsaved account name");
            assert_eq!(providers[0].credential, Some(reference));
            visual.update(|window, _| window.remove_window());
            fixture.close();
        }
    }
}
