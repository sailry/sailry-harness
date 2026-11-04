//! Controller interaction follows the public declaration; the Node owns authentication.
use super::*;
use sailry_protocol::plugin::model_auth::{Presentation, Redirect};

impl Login {
    pub(super) fn load_declaration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let client = self.binding.client.clone();
        let reference = self.provider.preset.clone();
        let authentication = self.provider.authentication;
        self.pending = true;
        self.error = None;
        let job = self.binding.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::ListChannelPresets {
                    refresh_partners: false,
                }))
                .await
        });
        self.declaration_task = Some(cx.spawn_in(window, async move |login, cx| {
            let result = job.await;
            let _ = login.update_in(cx, |login, window, cx| {
                login.pending = false;
                if login.closed || login.cancelling {
                    return;
                }
                login.presentation = match result {
                    Ok(Ok(Output::ChannelPresets(listing))) => listing.entries.into_iter()
                        .find(|entry| match &reference {
                            Some(reference) => entry.source == reference.source && entry.preset.id == reference.id,
                            None => matches!(entry.preset.auth, sailry_protocol::conversation::presets::Auth::Builtin { authentication: current } if current == authentication),
                        }).and_then(|entry| entry.login),
                    Ok(Err(error)) => {
                        login.error = Some(super::super::live::error_key(&error));
                        cx.notify();
                        return;
                    }
                    _ => {
                        login.error = Some("provider_query_failed");
                        cx.notify();
                        return;
                    }
                };
                if login.presentation.is_some() {
                    login.begin(window, cx);
                } else {
                    login.error = Some("provider_not_configured");
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn accepts_callback(&self) -> bool {
        manual(self.presentation.as_ref())
    }
}

fn manual(presentation: Option<&Presentation>) -> bool {
    matches!(
        presentation,
        Some(Presentation::Pkce {
            redirect: Redirect::Paste
        })
    ) || matches!(
        presentation,
        Some(Presentation::Script { callback: Some(callback) })
            if callback.manual && callback.port.is_none()
    )
}

pub(super) fn listener(
    presentation: Option<&Presentation>,
) -> Option<sailry_protocol::plugin::model_auth::Callback> {
    match presentation {
        Some(Presentation::Pkce {
            redirect: Redirect::Loopback,
        }) => Some(sailry_protocol::plugin::model_auth::Callback {
            path: "/callback".into(),
            port: None,
            redirect_uri: None,
            manual: false,
            allow_localhost: false,
        }),
        Some(Presentation::Script {
            callback: Some(callback),
        }) => Some(callback.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn declared_interaction() {
        for (scenario, check) in [
            (
                "keeps_fixed_listener_without_pasting",
                (|_scenario: &str| {
                    let callback = sailry_protocol::plugin::model_auth::Callback {
                        path: "/auth/callback".into(),
                        port: Some(1455),
                        redirect_uri: Some("http://localhost:1455/auth/callback".into()),
                        manual: true,
                        allow_localhost: true,
                    };
                    let presentation = Presentation::Script {
                        callback: Some(callback.clone()),
                    };
                    assert_eq!(
                        listener(Some(&presentation)),
                        Some(callback),
                        "keeps fixed listener without pasting"
                    );
                    assert!(
                        !manual(Some(&presentation)),
                        "keeps fixed listener without pasting"
                    );
                }) as fn(&str),
            ),
            (
                "uses_declared_interaction",
                (|_scenario: &str| {
                    for (presentation, needs_listener, needs_manual) in [
                        (
                            Presentation::Pkce {
                                redirect: Redirect::Loopback,
                            },
                            true,
                            false,
                        ),
                        (
                            Presentation::Pkce {
                                redirect: Redirect::Paste,
                            },
                            false,
                            true,
                        ),
                        (Presentation::Polling, false, false),
                        (Presentation::DeviceCode, false, false),
                        (Presentation::TokenExchange, false, false),
                        (Presentation::Script { callback: None }, false, false),
                    ] {
                        assert_eq!(
                            listener(Some(&presentation)).is_some(),
                            needs_listener,
                            "uses declared interaction"
                        );
                        assert_eq!(
                            manual(Some(&presentation)),
                            needs_manual,
                            "uses declared interaction"
                        );
                    }
                    for (path, manual_callback) in [("/auth/callback", false), ("/callback", true)]
                    {
                        let presentation = Presentation::Script {
                            callback: Some(sailry_protocol::plugin::model_auth::Callback {
                                path: path.into(),
                                port: None,
                                redirect_uri: None,
                                manual: manual_callback,
                                allow_localhost: manual_callback,
                            }),
                        };
                        let callback = listener(Some(&presentation)).unwrap();
                        assert_eq!(callback.path, path, "uses declared interaction");
                        assert_eq!(
                            callback.manual, manual_callback,
                            "uses declared interaction"
                        );
                        assert_eq!(
                            manual(Some(&presentation)),
                            manual_callback,
                            "uses declared interaction"
                        );
                    }
                    assert_eq!(listener(None), None, "uses declared interaction");
                    assert!(!manual(None), "uses declared interaction");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
