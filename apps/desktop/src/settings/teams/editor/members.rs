//! The member list: catalog roles in team order, reordered by dragging, and "Add role".
//! Removing a member only changes this team; the role stays in the shared catalog.
use super::*;
use crate::settings::teams::{draft, list};
use crate::ui::modal_list::{self, List, Row};
use gpui_kit::component::{popover::Popover, scroll::ScrollableElement as _};
use gpui_kit::prelude::FluentBuilder as _;
use sailry_protocol::role;

/// Drag payload and preview for reordering members.
#[derive(Clone)]
struct Drag {
    id: RoleId,
    label: SharedString,
}

impl Render for Drag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .px_3()
            .py_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .text_sm()
            .child(self.label.clone())
    }
}

impl Editor {
    fn role(&self, id: RoleId) -> Option<&role::Profile> {
        self.catalog.roles.iter().find(|role| role.id == id)
    }

    /// What a member does differently from the lead; nothing when it follows the lead.
    fn difference(&self, role: &role::Profile) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(model) = &role.model {
            parts.push(
                self.catalog
                    .models
                    .summary(Some(model), "teams_follow_lead")
                    .to_string(),
            );
        }
        if role.read_only {
            parts.push(tr("teams_read_only").to_string());
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }

    pub(super) fn member_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let locked = self.locked();
        let rows = self
            .members
            .iter()
            .filter_map(|&id| self.role(id))
            .map(|role| {
                let id = role.id;
                let (name, description) = draft::label(role);
                let row = Row::new(format!("team-member-{id}"), name.clone())
                    .leading(list::role_tile(role.appearance.as_ref(), cx))
                    .when(!description.is_empty(), |row| row.subtitle(description))
                    .when_some(self.difference(role), |row, meta| row.meta(meta));
                if self.builtin {
                    return row;
                }
                let row = row.remove(
                    tr("team_member_remove"),
                    cx.listener(move |editor, _, _, cx| editor.remove(id, cx)),
                );
                if locked {
                    return row;
                }
                row.draggable(
                    Drag {
                        id,
                        label: name.clone(),
                    },
                    cx.listener(move |editor, drag: &Drag, _, cx| editor.reorder(drag.id, id, cx)),
                )
            })
            .collect::<Vec<_>>();
        List::new("team-members")
            .heading(tr("team_members"))
            .when(!self.members.is_empty(), |list| {
                list.count(self.members.len().to_string())
            })
            .rows(rows)
            .when(!self.builtin, |list| {
                list.empty(tr("team_members_empty"))
                    .action(self.add_trigger(locked, cx))
            })
    }

    fn add_trigger(&self, locked: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let adder = self.adder.clone();
        let exhausted = self.candidates().is_empty();
        crate::theme::popover(Popover::new("team-add-role"), cx)
            .p_1p5()
            .anchor(Anchor::BottomLeft)
            .trigger_style(StyleRefinement::default().w_full())
            .trigger(
                modal_list::action("team-add-role-trigger", tr("team_add_role"))
                    .disabled(locked || exhausted),
            )
            .content(move |_, _, _| adder.clone())
    }

    /// Catalog roles outside the opened team, including those added since, with their state.
    fn candidates(&self) -> Vec<(role::Profile, bool)> {
        self.catalog
            .roles
            .iter()
            .filter_map(|role| {
                let member = self.members.contains(&role.id);
                (!(member && self.baseline.contains(&role.id))).then(|| (role.clone(), member))
            })
            .collect()
    }

    fn toggle(&mut self, id: RoleId, checked: bool, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        if !checked {
            self.remove(id, cx);
            return;
        }
        if !self.members.contains(&id) && self.role(id).is_some() {
            self.members.push(id);
            self.refresh(cx);
        }
    }

    fn remove(&mut self, id: RoleId, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        self.members.retain(|member| *member != id);
        self.refresh(cx);
    }

    /// The popover lists membership state, so it redraws with the member list.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.adder.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn reorder(&mut self, source: RoleId, target: RoleId, cx: &mut Context<Self>) {
        if self.locked() || source == target {
            return;
        }
        let position = |id| self.members.iter().position(|member| *member == id);
        let (Some(from), Some(to)) = (position(source), position(target)) else {
            return;
        };
        let member = self.members.remove(from);
        self.members.insert(to, member);
        cx.notify();
    }
}

/// The "Add role" popover content: a checklist of candidate roles.
pub(super) struct Adder {
    editor: WeakEntity<Editor>,
}

impl Adder {
    pub fn new(editor: WeakEntity<Editor>) -> Self {
        Self { editor }
    }
}

impl Render for Adder {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let candidates = self
            .editor
            .upgrade()
            .map(|editor| editor.read(cx).candidates())
            .unwrap_or_default();
        let rows = candidates
            .into_iter()
            .map(|(role, checked)| {
                let id = role.id;
                let editor = self.editor.clone();
                Row::new(format!("team-candidate-{id}"), draft::label(&role).0)
                    .checkbox(checked)
                    .leading(list::avatar(role.appearance.as_ref(), cx).xsmall())
                    .on_click(move |_, _, cx| {
                        _ = editor.update(cx, |editor, cx| editor.toggle(id, !checked, cx));
                    })
            })
            .collect::<Vec<_>>();
        div()
            .id("team-candidates")
            .w(px(320.))
            .max_h(px(280.))
            .overflow_y_scrollbar()
            .child(List::new("team-candidate-list").borderless().rows(rows))
    }
}
