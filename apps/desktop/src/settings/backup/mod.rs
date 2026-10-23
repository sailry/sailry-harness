use crate::feedback::Recovery as _;
mod editor;
mod records;
mod requests;
mod restore;
mod sessions;
mod view;
mod watch;

use super::{Workspace, providers::Binding};
use gpui_kit::{component::*, *};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Command, NodeId, Request,
    backup::{Page, Settings},
};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct State {
    panels: BTreeMap<NodeId, Entity<Panel>>,
    selected: Option<NodeId>,
}

pub(super) struct Panel {
    binding: Binding,
    active: bool,
    settings: Option<Settings>,
    page: Option<Page>,
    loading: bool,
    pending: bool,
    refresh: bool,
    rebase: bool,
    request: Option<Request>,
    stop: Option<CancellationToken>,
    generation: u64,
    editor: Option<Entity<editor::Editor>>,
    storage: Option<Entity<editor::Editor>>,
    sessions: Vec<sailry_protocol::Session>,
    records_open: bool,
    error: Option<&'static str>,
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
            |_, key, cx| {
                let owner = cx.weak_entity();
                notification::Notification::error(crate::tr(key)).scoped_action(move |_, _, cx| {
                    let owner = owner.clone();
                    let action = if matches!(key, "backup_unknown" | "backup_sessions_unknown") {
                        crate::feedback::retry("backup-retry")
                    } else {
                        button::Button::new("backup-retry").label(crate::tr("backup_reload"))
                    };
                    action
                        .debug_selector(|| "backup-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            _ = owner.update(cx, |panel, cx| {
                                if panel.request.is_some() {
                                    panel.retry(window, cx);
                                } else {
                                    panel.rebase = true;
                                    panel.load(window, cx);
                                }
                            });
                        }))
                })
            },
        );
        Self {
            binding,
            active: false,
            settings: None,
            page: None,
            loading: false,
            pending: false,
            refresh: false,
            rebase: false,
            request: None,
            stop: None,
            generation: 0,
            editor: None,
            storage: None,
            sessions: Vec::new(),
            records_open: false,
            error: None,
        }
    }
    fn locked(&self) -> bool {
        !self.active || self.pending || self.request.is_some() || self.settings.is_none()
    }
    fn deactivate(&mut self, cx: &mut Context<Self>) {
        self.active = false;
        self.editor = None;
        self.storage = None;
        self.generation += 1;
        if let Some(stop) = self.stop.take() {
            stop.cancel();
        }
        cx.notify();
    }
}

impl Workspace {
    pub(super) fn backup(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let binding = self.provider_link.as_ref().map(|live| live.binding.clone());
        let selected = binding.as_ref().map(|binding| binding.client.target());
        if self.backup.selected != selected {
            self.clear_backup(cx);
            self.backup.selected = selected;
        }
        let Some(binding) = binding else {
            return super::group::Group::new("backup_destination")
                .empty(IconName::Inbox, "knowledge_host_required")
                .into_any_element();
        };
        let panel = self
            .backup
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
    pub(super) fn clear_backup(&mut self, cx: &mut Context<Self>) {
        if let Some(node) = self.backup.selected.take()
            && let Some(panel) = self.backup.panels.get(&node)
        {
            panel.update(cx, |panel, cx| panel.deactivate(cx));
        }
    }
}

fn error_key(code: sailry_protocol::ErrorCode) -> &'static str {
    match code {
        sailry_protocol::ErrorCode::RevisionConflict | sailry_protocol::ErrorCode::Conflict => {
            "backup_conflict"
        }
        sailry_protocol::ErrorCode::InvalidRequest => "backup_invalid",
        sailry_protocol::ErrorCode::PermissionDenied => "backup_denied",
        sailry_protocol::ErrorCode::OutcomeUnknown => "backup_unknown",
        _ => "backup_failed",
    }
}
