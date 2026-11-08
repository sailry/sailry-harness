//! Kit 4424427 submenus accept plain labels, not model rows with score content.
//! The picker positions PopupMenu with Kit Popup; the menu owns input and
//! dismissal. Outer popovers delegate outside clicks to the surface
//! so that their rectangular hitboxes do not dismiss clicks on the submenu.
use super::*;
use gpui_kit::component::{
    menu::{PopupMenu, PopupMenuItem},
    *,
};
use sailry_protocol::Effort;
use std::{cell::Cell, rc::Rc};

pub(super) struct Efforts {
    pub selection: Selection,
    pub menu: Entity<PopupMenu>,
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    parent_focus: Option<FocusHandle>,
    _dismiss: Subscription,
}

impl Picker {
    pub(crate) fn surface(
        picker: Entity<Self>,
        popover: WeakEntity<PopoverState>,
        content: impl IntoElement,
    ) -> Stateful<Div> {
        div()
            .id("model-picker-surface")
            .child(content)
            .on_mouse_down_out(move |event, window, cx| {
                if picker
                    .read(cx)
                    .submenu
                    .as_ref()
                    .is_some_and(|submenu| submenu.bounds.get().contains(&event.position))
                {
                    return;
                }
                picker.update(cx, |picker, cx| {
                    picker.submenu = None;
                    cx.notify();
                });
                _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
            })
    }

    pub(super) fn open_efforts(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled
            || self
                .submenu
                .as_ref()
                .is_some_and(|submenu| submenu.selection == selection)
        {
            return;
        }
        let parent_focus = self
            .submenu
            .as_ref()
            .and_then(|submenu| submenu.parent_focus.clone())
            .or_else(|| window.focused(cx));
        if let Some(submenu) = self.submenu.take()
            && submenu.menu.focus_handle(cx).contains_focused(window, cx)
            && let Some(focus) = &parent_focus
        {
            focus.focus(window, cx);
        }
        let Some((_, model)) = self.choice(&selection) else {
            return;
        };
        if !self.effort_controls || model.efforts.is_empty() {
            cx.notify();
            return;
        }
        let selected = self.selected.as_ref() == Some(&selection);
        let current = scores::effective(
            selected,
            self.current_effort.or(self.selected_effort),
            model.effort,
        );
        let choices = model.efforts.clone();
        let owner = cx.entity();
        let picked = selection.clone();
        let focus = parent_focus.clone();
        let menu = PopupMenu::build(window, cx, move |menu, window, cx| {
            cx.observe(&owner, |_, _, cx| cx.notify()).detach();
            let restore = focus.clone();
            cx.on_release_in(window, move |menu, window, cx| {
                if menu.focus_handle(cx).contains_focused(window, cx)
                    && let Some(focus) = restore
                {
                    focus.focus(window, cx);
                }
            })
            .detach();
            let owner = owner.downgrade();
            choices.into_iter().fold(
                menu.min_w(px(144.))
                    .when_some(focus, |menu, focus| menu.action_context(focus)),
                |menu, effort| {
                    let scored = owner.clone();
                    let measured = picked.clone();
                    let select = owner.clone();
                    let picked = picked.clone();
                    menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            let score = scored.upgrade().and_then(|picker| {
                                let picker = picker.read(cx);
                                let (group, model) = picker.choice(&measured)?;
                                picker.exact(group, model, effort, cx)
                            });
                            // Kit items pad 8px; the selection fill spans the whole row.
                            div()
                                .w_full()
                                .min_h(px(26.))
                                .flex()
                                .items_center()
                                .mx(px(-8.))
                                .px(px(8.))
                                .rounded(cx.theme().radius)
                                .when(selected && current == effort, |row| {
                                    row.bg(cx.theme().primary.opacity(0.12))
                                })
                                .child(scores::effort_content(
                                    crate::reasoning::label(effort),
                                    score,
                                    cx,
                                ))
                        })
                        .on_click(move |_, window, cx| {
                            _ = select.update(cx, |picker, cx| {
                                picker.pick_effort(picked.clone(), effort, window, cx);
                            });
                        }),
                    )
                },
            )
        });
        menu.focus_handle(cx).focus(window, cx);
        let dismiss = cx.subscribe(&menu, |picker, menu, _: &DismissEvent, cx| {
            if picker
                .submenu
                .as_ref()
                .is_some_and(|submenu| submenu.menu == menu)
            {
                picker.submenu = None;
                cx.notify();
            }
        });
        self.submenu = Some(Efforts {
            selection,
            menu,
            bounds: Rc::new(Cell::new(Bounds::default())),
            parent_focus,
            _dismiss: dismiss,
        });
        cx.notify();
    }

    fn pick_effort(
        &mut self,
        selection: Selection,
        effort: Effort,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        if self.selected.as_ref() == Some(&selection) && self.current_effort.is_some() {
            self.submenu = None;
            cx.emit(EffortPicked(effort));
            if let Some(popover) = &self.popover {
                _ = popover.update(cx, |state, cx| state.dismiss(window, cx));
            }
            cx.notify();
        } else {
            self.pick(selection, effort, window, cx);
        }
    }
}
