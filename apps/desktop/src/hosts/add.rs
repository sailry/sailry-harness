//! One Add Host dialog retains the pairing and deployment drafts across methods.
use super::*;
use gpui_kit::component::{
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
};
use sailry_protocol::ssh::Authentication;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Method {
    #[default]
    Pairing,
    Password,
    PrivateKey,
    KeyFile,
    #[cfg(windows)]
    Wsl,
}

impl Method {
    const ALL: &'static [Self] = &[
        Self::Pairing,
        Self::Password,
        Self::PrivateKey,
        Self::KeyFile,
        #[cfg(windows)]
        Self::Wsl,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Pairing => "host_pairing_mode",
            Self::Password => "ssh_password",
            Self::PrivateKey => "ssh_private_key",
            Self::KeyFile => "ssh_key_file",
            #[cfg(windows)]
            Self::Wsl => "host_wsl",
        }
    }

    fn authentication(self) -> Option<Authentication> {
        match self {
            Self::Pairing => None,
            #[cfg(windows)]
            Self::Wsl => None,
            Self::Password => Some(Authentication::Password),
            Self::PrivateKey => Some(Authentication::PrivateKey),
            Self::KeyFile => Some(Authentication::KeyPath),
        }
    }
}

impl SearchableListItem for Method {
    type Value = Self;

    fn title(&self) -> SharedString {
        tr(self.label())
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

struct Add {
    method: Method,
    method_select: Entity<SelectState<Vec<Method>>>,
    pairing: Entity<crate::settings::Pairing>,
    installer: Entity<Installer>,
    #[cfg(windows)]
    wsl: Entity<wsl::Form>,
}

pub(crate) fn open(window: &mut Window, cx: &mut App) {
    let Some(services) = cx.try_global::<Services>().cloned() else {
        return;
    };
    let owner = cx.new(|cx| {
        let method_select = cx
            .new(|cx| SelectState::new(Method::ALL.to_vec(), Some(IndexPath::new(0)), window, cx));
        cx.subscribe_in(
            &method_select,
            window,
            |view: &mut Add, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(method)) = event {
                    view.select(*method, window, cx);
                }
            },
        )
        .detach();
        let pairing = cx.new(|cx| crate::settings::Pairing::new(window, cx));
        pairing.update(cx, |pairing, cx| pairing.prepare_network(window, cx));
        #[cfg(windows)]
        let wsl = cx.new(|cx| wsl::Form::new(&services, window, cx));
        #[cfg(windows)]
        cx.observe(&wsl, |_, _, cx| cx.notify()).detach();
        let installer = cx.new(|cx| Installer::new(services, None, window, cx));
        installer.update(cx, |installer, cx| {
            installer.network_draft = Some(pairing.clone());
            installer
                .editor
                .update(cx, |editor, _| editor.show_authentication = false);
        });
        cx.observe(&pairing, |_, _, cx| cx.notify()).detach();
        cx.observe(&installer, |_, _, cx| cx.notify()).detach();
        let editor = installer.read(cx).editor.clone();
        cx.observe(&editor, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&pairing, window, |_, _, _: &DismissEvent, window, cx| {
            window.close_dialog(cx)
        })
        .detach();
        Add {
            method: Method::default(),
            method_select,
            pairing,
            installer,
            #[cfg(windows)]
            wsl,
        }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let closing = owner.clone();
        dialog
            .form_title(tr("host_add"), cx)
            .w(px(420.).min(window.viewport_size().width - px(48.)))
            .max_h(window.viewport_size().height * 0.85)
            .overlay_closable(false)
            .on_close(move |_, _, cx| {
                closing.update(cx, |view, cx| {
                    view.pairing.update(cx, |pairing, cx| pairing.close(cx));
                    view.installer.update(cx, |installer, _| installer.cancel());
                })
            })
            .child(owner.clone())
            .footer(owner.update(cx, |view, cx| view.footer(cx)))
    });
}

impl Add {
    fn busy(&self, cx: &App) -> bool {
        let installer = self.installer.read(cx);
        self.pairing.read(cx).pairing_pending()
            || !installer.form
            || installer.editor.read(cx).blocked()
    }

    fn select(&mut self, method: Method, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy(cx) {
            self.method_select.update(cx, |select, cx| {
                select.set_selected_value(&self.method, window, cx);
            });
            return;
        }
        self.method = method;
        #[cfg(windows)]
        if method == Method::Wsl {
            self.wsl.update(cx, |form, cx| form.prepare(window, cx));
        }
        if let Some(authentication) = method.authentication() {
            let editor = self.installer.read(cx).editor.clone();
            editor.update(cx, |editor, cx| {
                editor.select_authentication(authentication, window, cx)
            });
        }
        cx.notify();
    }

    fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(windows)]
        if self.method == Method::Wsl {
            let installer = self.installer.read(cx);
            if !installer.form || installer.recovery.is_some() {
                return self
                    .installer
                    .update(cx, |installer, cx| installer.footer(cx));
            }
            let distribution = self.wsl.read(cx).distribution(cx);
            return DialogFooter::new()
                .w_full()
                .child(
                    Button::new("host-wsl-cancel")
                        .label(tr("settings_cancel"))
                        .on_click(cx.listener(|_, _, window, cx| window.close_dialog(cx))),
                )
                .child(
                    Button::new("host-wsl-install")
                        .primary()
                        .label(tr("host_install"))
                        .disabled(distribution.is_none() || self.busy(cx))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            let Some(distribution) = view.wsl.read(cx).distribution(cx) else {
                                return;
                            };
                            view.installer.update(cx, |installer, cx| {
                                installer.execute(
                                    Command::InstallWslHost {
                                        distribution,
                                        network: None,
                                    },
                                    window,
                                    cx,
                                )
                            });
                        })),
                )
                .into_any_element();
        }
        if self.method == Method::Pairing {
            self.pairing.update(cx, |pairing, cx| {
                pairing.pairing_footer(cx).into_any_element()
            })
        } else {
            self.installer
                .update(cx, |installer, cx| installer.footer(cx))
        }
    }
}

impl Render for Add {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy(cx);
        let method = Select::new(&self.method_select)
            .id("host-add-method")
            .w_full()
            .disabled(busy)
            .accessibility_label(tr("connection"));
        let body = if self.method == Method::Pairing {
            self.pairing
                .update(cx, |pairing, cx| pairing.pairing_body(cx))
                .into_any_element()
        } else {
            #[cfg(windows)]
            if self.method == Method::Wsl && self.installer.read(cx).form {
                self.wsl.update(cx, |form, cx| form.body(busy, cx))
            } else {
                self.installer.clone().into_any_element()
            }
            #[cfg(not(windows))]
            self.installer.clone().into_any_element()
        };
        v_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .child(
                self.pairing
                    .update(cx, |pairing, cx| pairing.target_network_fields(busy, cx)),
            )
            .child(method)
            .child(body)
    }
}
