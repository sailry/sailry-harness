use crate::feedback::Recovery as _;
mod index;
mod requests;
mod view;
mod watch;

use super::{Workspace, providers::Binding};
use gpui_kit::{
    component::{
        input::{InputEvent, InputState},
        searchable_list::SearchableListItem,
        select::{SelectEvent, SelectState},
        *,
    },
    *,
};
use index::Index;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, NodeId, Request};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<NodeId, Entity<Panel>>,
    selected: Option<NodeId>,
}

pub(super) struct Panel {
    binding: Binding,
    active: bool,
    python: Option<sailry_protocol::python::Settings>,
    index: Entity<InputState>,
    source: Entity<SelectState<Vec<Index>>>,
    pending: bool,
    loading: bool,
    refresh: bool,
    request: Option<Request>,
    stop: Option<CancellationToken>,
    generation: u64,
    error: Option<&'static str>,
    fault: Option<sailry_protocol::Fault>,
}

impl Drop for Panel {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
    }
}

impl Panel {
    fn new(binding: Binding, window: &mut Window, cx: &mut Context<Self>) -> Self {
        crate::feedback::observe_with(
            window,
            cx,
            |panel: &Self, _| panel.error.into_iter().collect(),
            |panel, key, cx| {
                let owner = cx.weak_entity();
                let notice = panel.fault.as_ref().map_or_else(
                    || notification::Notification::error(crate::tr(key)),
                    |fault| {
                        crate::feedback::diagnostic(fault.message.clone().into())
                            .title(crate::tr(key))
                    },
                );
                notice.scoped_action(move |_, _, cx| {
                    let owner = owner.clone();
                    crate::feedback::retry("runtime-retry")
                        .debug_selector(|| "runtime-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| {
                                if panel.request.is_some() {
                                    panel.retry(window, cx);
                                } else {
                                    panel.load(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        let index =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://pypi.org/simple/"));
        cx.subscribe_in(&index, window, |panel: &mut Self, _, event, window, cx| {
            if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                && !panel.locked()
                && panel.custom_index(cx)
            {
                let Some(mut settings) = panel.python.clone() else {
                    return;
                };
                let text = panel.index.read(cx).value().trim().to_owned();
                let value = (!text.is_empty()).then_some(text);
                if settings.index != value {
                    settings.index = value;
                    panel.submit(Command::SavePythonSettings(settings), window, cx);
                }
            }
        })
        .detach();
        let source = Self::index_selector(window, cx);
        Self {
            binding,
            active: false,
            python: None,
            index,
            source,
            pending: false,
            loading: false,
            refresh: false,
            request: None,
            stop: None,
            generation: 0,
            error: None,
            fault: None,
        }
    }

    pub(in crate::settings) fn locked(&self) -> bool {
        self.pending || self.request.is_some() || !self.active
    }
    fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.active = false;
        self.generation += 1;
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
        cx.notify();
    }
}

impl Workspace {
    pub(super) fn runtime(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let binding = self.provider_link.as_ref().map(|live| live.binding.clone());
        let selected = binding.as_ref().map(|binding| binding.client.target());
        if self.runtime.selected != selected {
            self.clear_runtime(cx);
            self.runtime.selected = selected;
        }
        let Some(binding) = binding else {
            return super::group::Group::new("runtime_python")
                .empty(IconName::Inbox, "knowledge_host_required")
                .into_any_element();
        };
        let panel = self
            .runtime
            .panels
            .entry(binding.client.target())
            .or_insert_with(|| cx.new(|cx| Panel::new(binding, window, cx)))
            .clone();
        panel.update(cx, |panel, cx| {
            if !panel.active {
                panel.active = true;
                panel.watch(window, cx);
                panel.load(window, cx);
            }
        });
        panel.into_any_element()
    }
    pub(super) fn clear_runtime(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = self.runtime.selected.take()
            && let Some(panel) = self.runtime.panels.get(&node)
        {
            panel.update(cx, |panel, cx| panel.deactivate(cx));
        }
    }
}

pub(super) fn error_key(code: sailry_protocol::ErrorCode) -> &'static str {
    match code {
        sailry_protocol::ErrorCode::RevisionConflict => "runtime_conflict",
        sailry_protocol::ErrorCode::InvalidRequest => "runtime_invalid",
        sailry_protocol::ErrorCode::PermissionDenied => "runtime_denied",
        sailry_protocol::ErrorCode::OutcomeUnknown => "runtime_unknown",
        _ => "runtime_failed",
    }
}
