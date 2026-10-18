//! Approval belongs to a provider and the package shown in this draft.
use super::*;
use gpui_kit::component::checkbox::Checkbox;
use sailry_protocol::conversation::{
    consent::{Approval, Scopes},
    presets::Entry,
};

impl Editor {
    pub(super) fn restore_consent(&mut self) {
        self.consent_scopes = self
            .selected
            .as_ref()
            .filter(|entry| self.saved_selection(entry))
            .and_then(|entry| {
                self.saved_consent
                    .as_ref()
                    .filter(|approval| same_origin(approval, entry))
            })
            .map_or_else(Scopes::default, |approval| approval.scopes);
        self.consent_changed = false;
    }

    pub(super) fn consent(&self) -> Option<Approval> {
        let entry = self.selected.as_ref()?;
        let package = entry.package.clone()?;
        if !self.consent_changed
            && let Some(approval) = self
                .saved_consent
                .as_ref()
                .filter(|approval| self.saved_selection(entry) && same_origin(approval, entry))
        {
            return Some(approval.clone());
        }
        Some(Approval {
            package,
            scopes: self.consent_scopes,
            origin: None,
        })
    }

    pub(super) fn can_login(&self) -> bool {
        self.uses_login()
            && self
                .selected
                .as_ref()
                .is_none_or(|entry| login_approved(entry.requested, self.consent_scopes))
    }

    fn saved_selection(&self, entry: &Entry) -> bool {
        same_preset(
            self.provider
                .as_ref()
                .and_then(|provider| provider.preset.as_ref()),
            entry,
        )
    }

    pub(super) fn consent_form(&self, cx: &Context<Self>) -> Div {
        let body = v_flex().w_full().gap_2();
        let Some(entry) = self
            .selected
            .as_ref()
            .filter(|entry| entry.package.is_some())
        else {
            return body;
        };
        let package = entry.package.as_ref().unwrap();
        let body = body.child(
            Field::new().label(tr("provider_package")).child(
                Button::new("provider-package-reference")
                    .ghost()
                    .small()
                    .label(package.name.clone())
                    .tooltip(format!(
                        "{}\n{}\n{}\n{}",
                        package.name,
                        package.digest,
                        package.settings_revision,
                        serde_json::to_string(&entry.origin).expect("package origins serialize")
                    )),
            ),
        );
        body.when(entry.requested.messages, |body| {
            body.child(
                Checkbox::new("provider-model-consent")
                    .text_sm()
                    .label(tr("provider_allow_messages"))
                    .checked(self.consent_scopes.messages)
                    .disabled(self.pending)
                    .on_click(cx.listener(|editor, checked, _, cx| {
                        editor.consent_scopes.messages = *checked;
                        editor.consent_changed = true;
                        cx.notify();
                    })),
            )
        })
        .when(entry.requested.credentials, |body| {
            body.child(
                Checkbox::new("provider-credential-consent")
                    .text_sm()
                    .label(tr("provider_allow_credentials"))
                    .checked(self.consent_scopes.credentials)
                    .disabled(self.pending)
                    .on_click(cx.listener(|editor, checked, _, cx| {
                        editor.consent_scopes.credentials = *checked;
                        editor.consent_changed = true;
                        cx.notify();
                    })),
            )
        })
    }
}

fn login_approved(requested: Scopes, approved: Scopes) -> bool {
    !requested.credentials || approved.credentials
}

fn same_preset(
    reference: Option<&sailry_protocol::conversation::presets::PresetRef>,
    entry: &Entry,
) -> bool {
    reference.is_some_and(|reference| {
        reference.source == entry.source && reference.id == entry.preset.id
    })
}

fn same_origin(approval: &Approval, entry: &Entry) -> bool {
    entry
        .package
        .as_ref()
        .is_some_and(|package| package.name == approval.package.name)
        && approval.origin.is_some()
        && approval.origin == entry.origin
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use sailry_protocol::{conversation::consent::Origin, plugin::Reference};

    #[test]
    fn gates_login_without_blocking_message_revocation() {
        let requested = Scopes {
            messages: true,
            credentials: true,
        };
        assert!(!login_approved(requested, Scopes::default()));
        assert!(!login_approved(
            requested,
            Scopes {
                messages: true,
                credentials: false
            }
        ));
        assert!(login_approved(
            requested,
            Scopes {
                messages: false,
                credentials: true
            }
        ));
        assert!(login_approved(
            Scopes {
                messages: true,
                credentials: false
            },
            Scopes::default()
        ));
    }

    #[test]
    fn preserves_scopes_only_for_same_source() {
        let mut entry = super::super::super::presets::bundled().remove(0);
        entry.source = sailry_protocol::conversation::presets::Source::Package {
            name: "sample-provider".into(),
        };
        entry.package = Some(Reference {
            name: "sample-provider".into(),
            digest: "b".repeat(64),
            settings_revision: 2,
        });
        entry.origin = Some(Origin::Git {
            repository: "https://example.test/models.git".into(),
            path: "provider".into(),
        });
        let mut approval = Approval {
            package: Reference {
                name: "sample-provider".into(),
                digest: "a".repeat(64),
                settings_revision: 1,
            },
            origin: entry.origin.clone(),
            scopes: Scopes {
                messages: true,
                credentials: false,
            },
        };
        assert!(same_origin(&approval, &entry));
        assert!(!approval.scopes.credentials);
        approval.origin = Some(Origin::Git {
            repository: "https://other.example/models.git".into(),
            path: "provider".into(),
        });
        assert!(!same_origin(&approval, &entry));
        approval.origin = entry.origin.clone();
        approval.package.name = "another-provider".into();
        assert!(!same_origin(&approval, &entry));
        let mut reference =
            super::super::super::presets::reference(&entry, &entry.preset.routes[0]);
        assert!(same_preset(Some(&reference), &entry));
        reference.id = "other-channel".into();
        assert!(!same_preset(Some(&reference), &entry));
        reference.id = entry.preset.id.clone();
        reference.source = sailry_protocol::conversation::presets::Source::Builtin;
        assert!(!same_preset(Some(&reference), &entry));
        assert!(!same_preset(None, &entry));
    }
}
