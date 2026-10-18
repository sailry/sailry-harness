//! One Add Host dialog retains the pairing and deployment drafts across methods.
use super::*;
use gpui_kit::component::tab::{Tab, TabBar};
use sailry_protocol::ssh::Authentication;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Method {
    #[default]
    Pairing,
    Password,
    PrivateKey,
    KeyFile,
}

impl Method {
    const ALL: [Self; 4] = [
        Self::Pairing,
        Self::Password,
        Self::PrivateKey,
        Self::KeyFile,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Pairing => "host_pairing_mode",
            Self::Password => "ssh_password",
            Self::PrivateKey => "ssh_private_key",
            Self::KeyFile => "ssh_key_file",
        }
    }

    fn authentication(self) -> Option<Authentication> {
        match self {
            Self::Pairing => None,
            Self::Password => Some(Authentication::Password),
            Self::PrivateKey => Some(Authentication::PrivateKey),
            Self::KeyFile => Some(Authentication::KeyPath),
        }
    }
}

struct Add {
    method: Method,
    pairing: Entity<crate::settings::Pairing>,
    installer: Entity<Installer>,
}

pub(crate) fn open(window: &mut Window, cx: &mut App) {
    let Some(services) = cx.try_global::<Services>().cloned() else {
        return;
    };
    let owner = cx.new(|cx| {
        let pairing = cx.new(|cx| crate::settings::Pairing::new(window, cx));
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
            pairing,
            installer,
        }
    });
    window.open_dialog(cx, move |dialog, window, cx| {
        let closing = owner.clone();
        dialog
            .form_title(tr("host_add"), cx)
            .w(px(560.).min(window.viewport_size().width - px(48.)))
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
            return;
        }
        self.method = method;
        if let Some(authentication) = method.authentication() {
            let editor = self.installer.read(cx).editor.clone();
            editor.update(cx, |editor, cx| {
                editor.select_authentication(authentication, window, cx)
            });
        }
        cx.notify();
    }

    fn footer(&self, cx: &mut Context<Self>) -> AnyElement {
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
        let tabs = TabBar::new("host-add-method")
            .segmented()
            .menu(false)
            .w_full()
            .selected_index(
                Method::ALL
                    .iter()
                    .position(|method| *method == self.method)
                    .unwrap_or_default(),
            )
            .children(Method::ALL.map(|method| {
                Tab::new()
                    .flex_1()
                    .min_w_0()
                    .label(tr(method.label()))
                    .disabled(busy)
            }))
            .on_click(cx.listener(|view, index, window, cx| {
                if let Some(method) = Method::ALL.get(*index) {
                    view.select(*method, window, cx);
                }
            }));
        let body = if self.method == Method::Pairing {
            self.pairing
                .update(cx, |pairing, cx| pairing.pairing_body(cx))
                .into_any_element()
        } else {
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
            .child(tabs)
            .child(body)
    }
}
