//! Host deployment credentials; reused from the SSH form at ef5e82ea (Apache-2.0).
use super::*;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    form::Field,
    input::{Input, InputState, Textarea, TextareaState},
    tab::{Tab, TabBar},
};
use sailry_protocol::{
    Secret,
    ssh::{Authentication, Credential},
};

mod key_file;

pub(crate) enum Event {
    Save(Box<Command>),
    Cancel,
}

pub(crate) struct Editor {
    original: Profile,
    pub(crate) sharing: Entity<super::sharing::Picker>,
    pub(crate) fields: [Entity<InputState>; 4],
    authentication: Authentication,
    pub(crate) secret: Entity<InputState>,
    key: Entity<TextareaState>,
    passphrase: Entity<InputState>,
    pub locked: bool,
    pub action: &'static str,
    invalid: bool,
    file: Option<key_file::Imported>,
    picking: bool,
    file_error: Option<&'static str>,
}
impl EventEmitter<Event> for Editor {}

impl Editor {
    pub(super) fn saved(&mut self, profile: Profile) {
        self.original = profile;
    }

    pub(crate) fn footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::component::dialog::DialogFooter::new()
            .w_full()
            .child(
                Button::new("ssh-edit-cancel")
                    .debug_selector(|| "ssh-edit-cancel".into())
                    .label(tr("settings_cancel"))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::Cancel))),
            )
            .child(
                Button::new("ssh-save")
                    .debug_selector(|| "ssh-save".into())
                    .primary()
                    .label(tr(self.action))
                    .disabled(self.locked || self.picking)
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            )
    }

    pub fn new(profile: Option<Profile>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe(window, cx, |view: &Self, _| {
            view.file_error
                .or(view.invalid.then_some("ssh_invalid"))
                .into_iter()
                .collect()
        });
        let profile = profile.unwrap_or_else(|| Profile {
            sharing: None,
            id: SshId::new(),
            revision: 0,
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            authentication: Authentication::Password,
            host_key: None,
        });
        let secret_hint = if profile.authentication == Authentication::KeyPath {
            "form_key_path_hint"
        } else {
            "form_password_hint"
        };
        let fields = [
            profile.name.clone(),
            profile.host.clone(),
            profile.port.to_string(),
            profile.username.clone(),
        ]
        .into_iter()
        .zip([
            "form_name_hint",
            "form_host_hint",
            "form_port_hint",
            "form_username_hint",
        ])
        .map(|(value, hint)| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(tr(hint))
            })
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
        Self {
            sharing: cx.new(|_| super::sharing::Picker::new(profile.sharing.as_ref())),
            authentication: profile.authentication,
            original: profile,
            fields,
            secret: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr(secret_hint))
            }),
            key: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(3, 6)
                    .placeholder(tr("form_private_key_hint"))
            }),
            passphrase: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr("form_passphrase_hint"))
            }),
            locked: false,
            action: "settings_save",
            invalid: false,
            file: None,
            picking: false,
            file_error: None,
        }
    }

    pub(super) fn save(&mut self, cx: &mut Context<Self>) {
        if self.locked || self.picking {
            return;
        }
        if self.authentication == Authentication::Agent
            || (self.authentication == Authentication::KeyPath
                && self.file.is_none()
                && (self.original.revision == 0
                    || self.original.authentication != Authentication::KeyPath))
        {
            self.invalid = true;
            cx.notify();
            return;
        }
        let fields = self
            .fields
            .each_ref()
            .map(|field| field.read(cx).value().to_string());
        let Ok(port) = fields[2].parse::<u16>() else {
            self.invalid = true;
            cx.notify();
            return;
        };
        if !self.sharing.read(cx).valid() {
            self.invalid = true;
            cx.notify();
            return;
        }
        let mut profile = self.original.clone();
        profile.sharing = self.sharing.read(cx).value();
        profile.name = fields[0].clone();
        profile.host = fields[1].clone();
        profile.port = port;
        profile.username = fields[3].clone();
        profile.authentication =
            if self.authentication == Authentication::KeyPath && self.file.is_some() {
                Authentication::PrivateKey
            } else {
                self.authentication
            };
        let secret = self.secret.read(cx).value().to_string();
        let key = self.key.read(cx).value().to_string();
        let passphrase = self.passphrase.read(cx).value().to_string();
        let passphrase = (!passphrase.is_empty()).then(|| Secret::new(passphrase));
        let changed = match self.authentication {
            Authentication::Password => !secret.is_empty(),
            Authentication::PrivateKey => !key.is_empty(),
            Authentication::KeyPath => self.file.is_some(),
            Authentication::Agent => false,
        };
        let replace = self.original.revision == 0
            || self.original.authentication != self.authentication
            || changed;
        let credential = replace.then(|| match self.authentication {
            Authentication::Password => Credential::Password {
                password: Secret::new(secret),
            },
            Authentication::PrivateKey => Credential::PrivateKey {
                key: Secret::new(key),
                passphrase,
            },
            Authentication::KeyPath => Credential::PrivateKey {
                key: self.file.as_ref().expect("selected key file").key.clone(),
                passphrase,
            },
            Authentication::Agent => Credential::Agent,
        });
        cx.emit(Event::Save(Box::new(Command::SaveSsh {
            expected_revision: profile.revision,
            profile,
            credential,
        })));
    }
}

pub(super) fn auth_label(authentication: Authentication) -> &'static str {
    match authentication {
        Authentication::Password => "ssh_password",
        Authentication::PrivateKey => "ssh_private_key",
        Authentication::KeyPath => "ssh_key_path",
        Authentication::Agent => "ssh_agent",
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut form = gpui_kit::component::form::Form::vertical();
        for (index, (key, input)) in ["settings_name", "ssh_host", "ssh_port", "ssh_username"]
            .into_iter()
            .zip(&self.fields)
            .enumerate()
        {
            form = form.child(
                Field::new().label(tr(key)).child(
                    div()
                        .debug_selector(move || format!("ssh-field-{index}"))
                        .child(Input::new(input).readonly(self.locked).aria_label(tr(key))),
                ),
            );
        }
        let methods = [
            Authentication::Password,
            Authentication::PrivateKey,
            Authentication::KeyPath,
        ];
        let tabs = TabBar::new("ssh-auth")
            .segmented()
            .equal_width()
            .menu(false)
            .w_full()
            .selected_index(
                methods
                    .iter()
                    .position(|method| *method == self.authentication)
                    .unwrap_or(usize::MAX),
            )
            .children(methods.into_iter().enumerate().map(|(index, auth)| {
                Tab::new()
                    .label(tr(match auth {
                        Authentication::KeyPath => "ssh_key_file",
                        Authentication::Agent => "ssh_agent_short",
                        _ => auth_label(auth),
                    }))
                    .disabled(self.locked || self.picking)
                    .debug_selector(move || format!("ssh-auth-{index}"))
            }))
            .on_click(cx.listener(move |this, &index, window, cx| {
                if this.locked || this.picking {
                    return;
                }
                this.authentication = methods[index];
                this.secret.update(cx, |input, cx| {
                    input.set_placeholder(
                        tr(if methods[index] == Authentication::KeyPath {
                            "form_key_path_hint"
                        } else {
                            "form_password_hint"
                        }),
                        window,
                        cx,
                    );
                });
                cx.notify();
            }));
        if self.authentication == Authentication::PrivateKey {
            form = form.child(
                Field::new().label(tr("ssh_private_key")).child(
                    div().debug_selector(|| "ssh-key".into()).child(
                        Textarea::new(&self.key)
                            .readonly(self.locked)
                            .aria_label(tr("ssh_private_key")),
                    ),
                ),
            );
        } else if self.authentication == Authentication::KeyPath {
            form = form.child(
                Field::new()
                    .label(tr("ssh_key_path"))
                    .child(self.key_file_button(cx)),
            );
        } else if self.authentication != Authentication::Agent {
            form = form.child(
                Field::new()
                    .label(tr(auth_label(self.authentication)))
                    .child(
                        div().debug_selector(|| "ssh-secret".into()).child(
                            Input::new(&self.secret)
                                .readonly(self.locked)
                                .aria_label(tr(auth_label(self.authentication)))
                                .mask_toggle(),
                        ),
                    ),
            );
        }
        if matches!(
            self.authentication,
            Authentication::PrivateKey | Authentication::KeyPath
        ) {
            form = form.child(
                Field::new().label(tr("ssh_passphrase")).child(
                    Input::new(&self.passphrase)
                        .readonly(self.locked)
                        .aria_label(tr("ssh_passphrase"))
                        .mask_toggle(),
                ),
            );
        }
        self.sharing
            .update(cx, |sharing, _| sharing.locked = self.locked);
        v_flex()
            .gap_4()
            .child(div().debug_selector(|| "ssh-auth-tabs".into()).child(tabs))
            .child(form)
            .child(self.sharing.clone())
            .when(
                self.original.revision > 0 && self.authentication != Authentication::Agent,
                |this| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("ssh_keep_credential")),
                    )
                },
            )
    }
}
