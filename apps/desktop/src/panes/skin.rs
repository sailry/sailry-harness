//! Kit's renderer seam supplies split-pane headers without tab merging.
//! The default skin closes views synchronously and drags to tab groups; terminals
//! require confirmed Node closure and the workspace uses center replacement.
use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::prelude::FluentBuilder as _;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) struct Skin {
    kit: Rc<DockSkin>,
}
impl Skin {
    pub fn new(cx: &mut Context<DockArea>) -> Rc<Self> {
        Rc::new(Self {
            kit: DockSkin::new(cx),
        })
    }
}
impl DockAreaRenderer for Skin {
    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(Group {
            kit: self.kit.tab_group_renderer(),
            current: RefCell::new(None),
            dragged: Rc::new(Cell::new(None)),
        })
    }
    fn render_split_handle(
        &self,
        handle: &gpui_kit::base::ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.kit.render_split_handle(handle, window, cx)
    }
}
struct Group {
    kit: Rc<dyn TabGroupRenderer>,
    current: RefCell<Option<Entity<Pane>>>,
    dragged: Rc<Cell<Option<Target>>>,
}
impl TabGroupRenderer for Group {
    fn frame(&self, group: &TabGroupContext, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        let pane = group
            .active_panel()
            .and_then(|panel| panel.as_any().downcast_ref::<Entity<Pane>>());
        *self.current.borrow_mut() = pane.cloned();
        let dragged = self.dragged.clone();
        let focus = pane.map(|pane| {
            let pane = pane.read(cx);
            (pane.focus.clone(), pane.target, pane.owner.clone())
        });
        div()
            .id("workspace-pane")
            .when_some(focus, |view, (focus, target, owner)| {
                view.track_focus(&focus)
                    .key_context("WorkspacePane")
                    .on_action(move |_: &crate::shell::shortcuts::CloseFocused, _, cx| {
                        let _ = owner.update(cx, |_, cx| cx.emit(Event::RequestClose(target)));
                    })
            })
            .on_drag_move(
                move |event: &DragMoveEvent<gpui_kit::component::dock::AnyDrag>, _, cx| {
                    dragged.set(
                        event
                            .drag(cx)
                            .value()
                            .downcast_ref::<Drag>()
                            .map(|drag| drag.target),
                    );
                },
            )
    }
    fn render_tab_bar(&self, group: &TabGroupContext, _: &mut Window, cx: &mut App) -> AnyElement {
        let Some(pane) = group
            .active_panel()
            .and_then(|panel| panel.as_any().downcast_ref::<Entity<Pane>>())
        else {
            return div().into_any_element();
        };
        let pane = pane.read(cx);
        let target = pane.target;
        let title = pane.title.clone();
        let owner = pane.owner.clone();
        let multiple = owner
            .upgrade()
            .map(|owner| {
                let owner = owner.read(cx);
                let workspace = owner.workspace(target, cx).unwrap();
                owner.members(workspace, cx).len() > 1
            })
            .unwrap_or(false);
        if !multiple {
            return div().into_any_element();
        }
        crate::header::Header::new("pane-header", cx)
            .border_b_0()
            .px_3()
            .gap_1()
            .child(
                h_flex()
                    .id("pane-title")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_3()
                    .debug_selector(move || format!("pane-title-{target:?}"))
                    .child(
                        div()
                            .debug_selector(move || format!("pane-caption-{target:?}"))
                            .w_full()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(title),
                    ),
            )
            .child(pane.actions(multiple))
            .into_any_element()
    }
    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        if let (Some(pane), Some(source)) = (self.current.borrow().as_ref(), self.dragged.get()) {
            let pane = pane.read(cx);
            if pane.target == source {
                return None;
            }
            if let Some(owner) = pane.owner.upgrade() {
                let owner = owner.read(cx);
                let workspace = owner.workspace(pane.target, cx)?;
                if !owner.can_drop(workspace, source, indicator.placement(), cx) {
                    return None;
                }
            }
        }
        self.kit.render_drop_indicator(indicator, window, cx)
    }
}

impl Pane {
    pub(super) fn actions(&self, multiple: bool) -> AnyElement {
        let pane = self;
        let target = pane.target;
        let drag = Drag::panel(target, pane.title.clone()).payload();
        let focus_owner = pane.owner.clone();
        let close_owner = pane.owner.clone();
        h_flex()
            .gap_1()
            .flex_shrink_0()
            .when(matches!(target, Target::Session(..)), |header| {
                header.children(pane.search.clone())
            })
            .when(multiple, |header| {
                header.child(
                    // Kit Button has no drag source API; its hitbox owns the drag gesture.
                    div()
                        .id("pane-drag")
                        .debug_selector(move || format!("pane-drag-{target:?}"))
                        .when(!pane.closing, |handle| {
                            handle
                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    cx.stop_propagation();
                                    let _ = focus_owner.update(cx, |owner, cx| {
                                        owner.focus(target, cx);
                                        cx.emit(Event::Activate(target));
                                    });
                                })
                                .on_drag(drag, |drag, _, _, cx| {
                                    cx.new(|_| drag.value().downcast_ref::<Drag>().unwrap().clone())
                                })
                        })
                        .child(
                            Button::new("pane-drag-button")
                                .ghost()
                                .small()
                                .cursor_grab()
                                .icon(Icon::default().path(crate::assets::DRAG_ICON))
                                .disabled(pane.closing)
                                .tooltip(crate::tr("pane_drag"))
                                .accessibility_label(crate::tr("pane_drag")),
                        ),
                )
            })
            .when(
                multiple || matches!(target, Target::Terminal(..)),
                |header| {
                    header.child(
                        Button::new("pane-close")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .debug_selector(move || format!("pane-close-{target:?}"))
                            .disabled(pane.closing)
                            .accessibility_label(crate::tr("close"))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let _ =
                                    close_owner.update(cx, |_, cx| cx.emit(Event::Close(target)));
                            }),
                    )
                },
            )
            .into_any_element()
    }
}
