//! Plugins mount the shared Rust terminal against an immutable Node/worktree scope.
use crate::terminal::{Binding, Scope as TerminalScope, View};
use gpui_kit::{component::*, *};
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::{TerminalId, plugin, terminal::Status};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::Arc,
};

pub(super) struct Scope {
    pub binding: crate::conversation::live::Binding,
    pub snapshot: super::panel::Snapshot,
    pub context: plugin::Context,
    pub ssh: bool,
    pub terminals: bool,
}

#[derive(Clone, Default)]
pub(super) struct Terminals {
    views: Rc<RefCell<Views>>,
    focus: Rc<Cell<Option<TerminalId>>>,
}

type Views = BTreeMap<String, (TerminalId, Entity<View>)>;

impl Terminals {
    pub fn focus(&self, id: TerminalId, window: &mut Window, cx: &mut App) {
        if let Some((_, view)) = self
            .views
            .borrow()
            .values()
            .find(|(terminal, _)| *terminal == id)
        {
            view.update(cx, |view, cx| view.focus(window, cx));
        } else {
            self.focus.set(Some(id));
        }
    }

    pub fn clear_focus(&self) {
        self.focus.set(None);
    }

    pub fn module(
        &self,
        module: HostModule,
        scope: Option<Scope>,
        host: Arc<super::host::Host>,
    ) -> HostModule {
        let stop = host.stop_token();
        let mounted = self.clone();
        let mounted_host = host.clone();
        let focus = self.focus.clone();
        let declarations = format!(
            "{}\nexport function focusTerminal(terminal:string):void;",
            module.declared().unwrap_or_default()
        );
        module
            .function("focusTerminal", move |args| {
                host.check()?;
                let id = args
                    .string(0)?
                    .parse::<TerminalId>()
                    .map_err(|_| HostError::new("invalid terminal ID"))?;
                focus.set(Some(id));
                Ok(HostValue::Null)
            })
            .component("Terminal", move |args, window, cx| {
                let id = args
                    .props()
                    .get("terminal")
                    .and_then(|value| value.as_str())
                    .and_then(|value| value.parse::<TerminalId>().ok());
                let Some(id) = id else {
                    mounted.views.borrow_mut().remove(args.id());
                    return div().into_any_element();
                };
                let scope = scope
                    .as_ref()
                    .filter(|_| !stop.is_cancelled())
                    .filter(|scope| {
                        scope.snapshot.borrow().as_ref().is_some_and(|snapshot| {
                            snapshot.node == scope.binding.client.target()
                                && snapshot.plugins.iter().any(|package| {
                                    package.enabled && package.reference() == scope.context.package
                                })
                                && {
                                    snapshot.terminals.iter().any(|info| {
                                        info.id == id
                                            && ((scope.ssh && info.ssh.is_some())
                                                || (scope.terminals
                                                    && info.ssh.is_none()
                                                    && info.worktree == scope.context.worktree))
                                            && info.status != Status::Closed
                                    })
                                }
                        })
                    });
                let Some(scope) = scope else {
                    mounted.views.borrow_mut().remove(args.id());
                    return div()
                        .text_color(cx.theme().muted_foreground)
                        .debug_selector(|| "plugin-terminal-unavailable".into())
                        .child(crate::tr("plugins_view_unavailable"))
                        .into_any_element();
                };
                let old = mounted.views.borrow().get(args.id()).cloned();
                let terminal = if let Some((_, view)) = old.filter(|(selected, _)| *selected == id)
                {
                    view
                } else {
                    let binding = Binding {
                        client: scope.binding.client.clone(),
                        caller: scope.binding.defaults.target(),
                        id,
                        runtime: scope.binding.runtime.handle().clone(),
                        scope: Some(TerminalScope {
                            context: scope.context.clone(),
                            stop: stop.clone(),
                        }),
                    };
                    let view = cx.new(|cx| View::new(binding, window, cx));
                    mounted
                        .views
                        .borrow_mut()
                        .insert(args.id().into(), (id, view.clone()));
                    view
                };
                if mounted.focus.get() == Some(id) {
                    mounted.focus.set(None);
                    let terminal = terminal.clone();
                    let host = mounted_host.clone();
                    window.defer(cx, move |window, cx| {
                        if host.check().is_ok() && !window.has_active_dialog(cx) {
                            terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
                        }
                    });
                }
                div()
                    .size_full()
                    .min_w_0()
                    .min_h_0()
                    .debug_selector(|| "plugin-terminal".into())
                    .child(terminal)
                    .into_any_element()
            })
            .declarations(declarations)
    }

    #[cfg(test)]
    pub fn first(&self) -> Option<Entity<View>> {
        self.views
            .borrow()
            .values()
            .next()
            .map(|(_, view)| view.clone())
    }

    #[cfg(test)]
    pub fn focused(&self, id: TerminalId, window: &Window, cx: &App) -> bool {
        self.views.borrow().values().any(|(terminal, view)| {
            *terminal == id && view.read(cx).focus_handle(cx).is_focused(window)
        })
    }

    #[cfg(test)]
    pub fn focus_state(&self, cx: &App) -> (Option<TerminalId>, Vec<(TerminalId, FocusHandle)>) {
        (
            self.focus.get(),
            self.views
                .borrow()
                .values()
                .map(|(id, view)| (*id, view.read(cx).focus_handle(cx)))
                .collect(),
        )
    }
}
