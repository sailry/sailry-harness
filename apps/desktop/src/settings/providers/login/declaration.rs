//! Controller interaction follows the public declaration; the Node owns authentication.
use super::*;
use sailry_protocol::{
    Authentication,
    plugin::model_auth::{Presentation, Redirect},
};

impl Login {
    pub(super) fn load_declaration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending || self.closed {
            return;
        }
        let client = self.binding.client.clone();
        let reference = self.provider.preset.clone();
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
                    Ok(Ok(Output::ChannelPresets(listing))) => reference
                        .as_ref()
                        .and_then(|reference| {
                            listing.entries.into_iter().find(|entry| {
                                entry.source == reference.source && entry.preset.id == reference.id
                            })
                        })
                        .and_then(|entry| entry.login),
                    _ => None,
                };
                if login.presentation.is_some() {
                    login.begin(window, cx);
                } else {
                    login.error = Some("provider_account_unavailable");
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn accepts_callback(&self) -> bool {
        manual(self.provider.authentication, self.presentation)
    }
}

fn manual(authentication: Authentication, presentation: Option<Presentation>) -> bool {
    authentication == Authentication::ClaudeOAuth
        || (authentication == Authentication::Plugin
            && matches!(
                presentation,
                Some(Presentation::Pkce {
                    redirect: Redirect::Paste
                })
            ))
}

pub(super) fn listener(
    authentication: Authentication,
    presentation: Option<Presentation>,
) -> Option<(&'static str, bool)> {
    match authentication {
        Authentication::ChatGpt => Some(("/auth/callback", true)),
        Authentication::ClaudeOAuth => Some(("/callback", false)),
        Authentication::Plugin
            if matches!(
                presentation,
                Some(Presentation::Pkce {
                    redirect: Redirect::Loopback
                })
            ) =>
        {
            Some(("/callback", true))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn uses_declared_interaction() {
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
        ] {
            assert_eq!(
                listener(Authentication::Plugin, Some(presentation)).is_some(),
                needs_listener
            );
            assert_eq!(
                manual(Authentication::Plugin, Some(presentation)),
                needs_manual
            );
        }
        assert_eq!(
            listener(Authentication::ChatGpt, None),
            Some(("/auth/callback", true))
        );
        assert_eq!(
            listener(Authentication::ClaudeOAuth, None),
            Some(("/callback", false))
        );
        assert!(manual(Authentication::ClaudeOAuth, None));
        assert!(!manual(Authentication::Copilot, None));
        assert_eq!(listener(Authentication::ChatGptOAuth, None), None);
    }
}
