//! The member column: the pinned lead, role rows ordered by dragging, and "Add role".
//! Removing a member only changes this team; the role stays in the shared catalog.
use super::*;
use crate::settings::teams::{draft, list};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    avatar::Avatar,
    checkbox::Checkbox,
    list::ListItem,
    popover::{Popover, PopoverState},
    separator::Separator,
};
use gpui_kit::prelude::FluentBuilder as _;

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

fn lead_avatar(cx: &App) -> Avatar {
    Avatar::new().placeholder(
        Icon::from(gpui_kit::assets::IconName::Crown).text_color(cx.theme().muted_foreground),
    )
}

/// Row text for a member: its draft values when opened, else the catalog role.
struct Line {
    name: SharedString,
    appearance: Option<Appearance>,
    model: Option<role::Model>,
    read_only: bool,
}

impl Editor {
    fn line(&self, member: &Member, cx: &App) -> Line {
        match (&member.form, &member.original) {
            (Some(form), _) => {
                let form = form.read(cx);
                Line {
                    name: form.name(cx),
                    appearance: form.appearance().cloned(),
                    model: form.model(cx),
                    read_only: form.read_only(),
                }
            }
            (None, Some(role)) => Line {
                name: draft::label(role).0,
                appearance: role.appearance.clone(),
                model: role.model.clone(),
                read_only: role.read_only,
            },
            (None, None) => Line {
                name: SharedString::default(),
                appearance: None,
                model: None,
                read_only: false,
            },
        }
    }

    pub(super) fn column(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let locked = self.locked();
        let lead = self.lead.read(cx).value().cloned();
        let rows = self
            .members
            .iter()
            .map(|member| self.member_row(member, locked, cx))
            .collect::<Vec<_>>();
        v_flex()
            .id("team-members")
            .w(px(240.))
            .flex_shrink_0()
            .h_full()
            .gap_0p5()
            .pr_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .overflow_y_scrollbar()
            .child(
                div()
                    .pb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("team_members")),
            )
            .child(
                self.row(
                    "team-lead".into(),
                    lead_avatar(cx),
                    tr("team_lead"),
                    self.catalog
                        .models
                        .summary(lead.as_ref(), "team_follow_conversation"),
                    self.selected == Selected::Lead,
                    cx,
                )
                .on_click(
                    cx.listener(|editor, _, window, cx| editor.select(Selected::Lead, window, cx)),
                ),
            )
            .children(rows)
            .when(!self.builtin, |column| {
                column.child(self.add_trigger(locked, cx))
            })
    }

    fn row(
        &self,
        id: SharedString,
        avatar: Avatar,
        name: SharedString,
        detail: SharedString,
        selected: bool,
        cx: &App,
    ) -> ListItem {
        ListItem::new(id)
            .w_full()
            .px_2()
            .py_1p5()
            .rounded(cx.theme().radius)
            .selected(selected)
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_3()
                    .child(avatar.with_size(px(32.)))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().truncate().child(name))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(detail),
                            ),
                    ),
            )
    }

    fn member_row(&self, member: &Member, locked: bool, cx: &mut Context<Self>) -> AnyElement {
        let id = member.id;
        let line = self.line(member, cx);
        let name = if line.name.is_empty() {
            tr("role_new")
        } else {
            line.name.clone()
        };
        let mut detail = self
            .catalog
            .models
            .summary(line.model.as_ref(), "teams_follow_lead")
            .to_string();
        if line.read_only {
            detail.push_str(" · ");
            detail.push_str(&tr("teams_read_only"));
        }
        let selected = self.selected == Selected::Member(id);
        let group = SharedString::from(format!("team-member-{id}"));
        let remove = Button::new(SharedString::from(format!("team-member-remove-{id}")))
            .ghost()
            .xsmall()
            .icon(IconName::Close)
            .tooltip(tr("team_member_remove"))
            .accessibility_label(tr("team_member_remove"))
            .when(!selected, |button| {
                button
                    .opacity(0.)
                    .group_hover(group.clone(), |style| style.opacity(1.))
            })
            .on_click(cx.listener(move |editor, _, _, cx| {
                cx.stop_propagation();
                editor.remove(id, cx);
            }));
        let drag = Drag {
            id,
            label: name.clone(),
        };
        self.row(
            group.clone(),
            list::avatar(line.appearance.as_ref(), cx),
            name,
            detail.into(),
            selected,
            cx,
        )
        .group(group)
        .on_click(
            cx.listener(move |editor, _, window, cx| {
                editor.select(Selected::Member(id), window, cx)
            }),
        )
        .when(!locked, |row| {
            row.child(remove)
                .on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
                .drag_over::<Drag>(|style, _, _, cx| style.bg(cx.theme().accent))
                .on_drop(
                    cx.listener(move |editor, drag: &Drag, _, cx| editor.reorder(drag.id, id, cx)),
                )
        })
        .into_any_element()
    }

    fn add_trigger(&self, locked: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let adder = self.adder.clone();
        crate::theme::popover(Popover::new("team-add-role"), cx)
            .p_2()
            .anchor(Anchor::TopLeft)
            .trigger_style(StyleRefinement::default().w_full())
            .trigger(
                Button::new("team-add-role-trigger")
                    .outline()
                    .w_full()
                    .icon(IconName::Plus)
                    .label(tr("team_add_role"))
                    .disabled(locked),
            )
            .content(move |_, _, cx| {
                let popover = cx.entity().downgrade();
                adder.update(cx, |adder, _| adder.popover = Some(popover));
                adder.clone()
            })
    }

    /// Catalog roles outside the opened team, including those added since, with their state.
    fn candidates(&self) -> Vec<(role::Profile, bool)> {
        self.catalog
            .roles
            .iter()
            .filter_map(|role| {
                let member = self.member(role.id).is_some();
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
        if self.member(id).is_none()
            && let Some(role) = self.catalog.roles.iter().find(|role| role.id == id)
        {
            self.members.push(Member {
                id,
                original: Some(role.clone()),
                form: None,
            });
            self.refresh(cx);
        }
    }

    /// Adds a blank member and selects it; the role is created in the catalog on Save.
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        let id = RoleId::new();
        let form = self.mount(form::blank(id), true, window, cx);
        self.members.push(Member {
            id,
            original: None,
            form: Some(form),
        });
        self.select(Selected::Member(id), window, cx);
        self.refresh(cx);
    }

    fn remove(&mut self, id: RoleId, cx: &mut Context<Self>) {
        if self.locked() {
            return;
        }
        self.members.retain(|member| member.id != id);
        if self.selected == Selected::Member(id) {
            self.selected = Selected::Lead;
        }
        self.refresh(cx);
    }

    /// The popover lists membership state, so it redraws with the member column.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.adder.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    fn reorder(&mut self, source: RoleId, target: RoleId, cx: &mut Context<Self>) {
        if self.locked() || source == target {
            return;
        }
        let position = |id| self.members.iter().position(|member| member.id == id);
        let (Some(from), Some(to)) = (position(source), position(target)) else {
            return;
        };
        let member = self.members.remove(from);
        self.members.insert(to, member);
        cx.notify();
    }
}

/// The "Add role" popover content: a checkbox per candidate role and "New role".
pub(super) struct Adder {
    editor: WeakEntity<Editor>,
    pub popover: Option<WeakEntity<PopoverState>>,
}

impl Adder {
    pub fn new(editor: WeakEntity<Editor>) -> Self {
        Self {
            editor,
            popover: None,
        }
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
            .map(|(role, checked)| (draft::label(&role).0, role, checked))
            .enumerate()
            .map(|(index, (name, role, checked))| {
                let editor = self.editor.clone();
                let id = role.id;
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        Checkbox::new(("team-candidate", index))
                            .checked(checked)
                            .accessibility_label(name.clone())
                            .on_click(move |checked: &bool, _, cx| {
                                _ = editor.update(cx, |editor, cx| editor.toggle(id, *checked, cx));
                            }),
                    )
                    .child(list::avatar(role.appearance.as_ref(), cx).xsmall())
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
            })
            .collect::<Vec<_>>();
        let editor = self.editor.clone();
        let popover = self.popover.clone();
        v_flex()
            .w(px(280.))
            .gap_1()
            .child(
                v_flex()
                    .id("team-candidates")
                    .max_h(px(240.))
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .child(Separator::horizontal())
            .child(
                Button::new("team-role-new")
                    .ghost()
                    .w_full()
                    .justify_start()
                    .icon(IconName::Plus)
                    .label(tr("role_new"))
                    .on_click(move |_, window, cx| {
                        _ = editor.update(cx, |editor, cx| editor.create(window, cx));
                        if let Some(popover) = &popover {
                            _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
                        }
                    }),
            )
    }
}
