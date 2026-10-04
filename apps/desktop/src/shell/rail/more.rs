//! Kit menu rows keep navigation and pinning as independent actions.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu, PopupMenuItem};
use std::{cell::Cell, rc::Rc};

impl Shell {
    pub(super) fn more_features(
        &self,
        features: Vec<Feature>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        let selected = features
            .iter()
            .any(|feature| !feature.pinned(cx) && feature.selected(self, cx));
        Button::new("feature-more")
            .debug_selector(|| "navigation-more".into())
            .ghost()
            .when(!cx.theme().is_dark(), |button| {
                button.custom(crate::theme::subtle_button(cx))
            })
            .size_9()
            .icon(IconName::Ellipsis)
            .selected(selected)
            .tooltip(tr("features_all"))
            .accessibility_label(tr("features_all"))
            .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, cx| {
                populate(menu, &features, owner.clone(), cx)
            })
            .anchor_offset(Self::rail_popover_offset(window))
    }
}

fn populate(
    mut menu: PopupMenu,
    features: &[Feature],
    owner: WeakEntity<Shell>,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu_entity = cx.weak_entity();
    menu = menu.min_w(px(240.)).max_h(px(480.)).scrollable(true);
    for feature in features {
        let feature = feature.clone();
        let open_feature = feature.clone();
        let open_owner = owner.clone();
        let drop_owner = owner.clone();
        let menu_entity = menu_entity.clone();
        let edge = Rc::new(Cell::new(None));
        menu = menu.item(
            PopupMenuItem::element(move |_, cx| {
                let feature = feature.clone();
                let pinned = feature.pinned(cx);
                let moving = feature.clone();
                let target = feature.key.clone();
                let last = feature.last();
                let drag_edge = edge.clone();
                let drop_edge = edge.clone();
                let drop_owner = drop_owner.clone();
                let menu_entity = menu_entity.clone();
                let key = feature.key.clone();
                let pin_selector = format!("feature-pin-{key}");
                let label = tr(if pinned {
                    "feature_unpin"
                } else {
                    "feature_pin"
                });
                h_flex()
                    .id(SharedString::from(format!("feature-row-{key}")))
                    .group("feature-menu-row")
                    .debug_selector({
                        let selector = format!("more-{}", feature.selector());
                        move || selector.clone()
                    })
                    .w_full()
                    .gap_3()
                    .relative()
                    .when_some(edge.get().filter(|_| cx.has_active_drag()), |row, after| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .h_1()
                                .rounded_full()
                                // Drop-target fills are translucent; an insertion edge needs solid ink.
                                .bg(cx.theme().primary)
                                .when(after, |line| line.bottom_neg_1())
                                .when(!after, |line| line.top_neg_1())
                                .child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_neg_0p5()
                                        .size_2()
                                        .rounded_full()
                                        .bg(cx.theme().primary),
                                ),
                        )
                    })
                    .on_drag_move(move |event: &DragMoveEvent<order::Drag>, _, cx| {
                        let position = event.event.position;
                        let edge = (event.bounds.contains(&position)
                            && event.drag(cx).0.key != moving.key)
                            .then_some(!last && position.y >= event.bounds.center().y);
                        if drag_edge.replace(edge) != edge {
                            cx.refresh_windows();
                        }
                    })
                    .on_drop(move |drag: &order::Drag, window, cx| {
                        cx.stop_propagation();
                        let Some(after) = drop_edge.take() else {
                            return;
                        };
                        let Some(owner) = drop_owner.upgrade() else {
                            return;
                        };
                        let features = owner.read(cx).features(cx);
                        if order::move_feature(&features, &drag.0.key, &target, after, cx) {
                            let features = owner.read(cx).features(cx);
                            let owner = owner.downgrade();
                            let _ = menu_entity.update(cx, |menu, cx| {
                                menu.rebuild(window, cx, |menu, _, cx| {
                                    populate(menu, &features, owner, cx)
                                });
                            });
                        }
                    })
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("feature-drag-{key}")))
                            .debug_selector({
                                let key = key.clone();
                                move || format!("feature-drag-{key}")
                            })
                            .flex_1()
                            .min_w_0()
                            .gap_3()
                            .child(feature.icon.clone().size_4())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(feature.label.clone()),
                            )
                            .when(!last, |row| {
                                row.on_drag(order::Drag(feature.clone()), |drag, _, _, cx| {
                                    cx.new(|_| drag.clone())
                                })
                            }),
                    )
                    .when(!feature.fixed(), |row| {
                        row.child(
                            Button::new(SharedString::from(format!("pin-{key}")))
                                .debug_selector(move || pin_selector.clone())
                                .ghost()
                                .when(!cx.theme().is_dark(), |button| {
                                    button.custom(crate::theme::subtle_button(cx))
                                })
                                .xsmall()
                                .icon(Icon::default().path("reicon:map/pin-tack"))
                                .selected(pinned)
                                .opacity(if pinned { 1. } else { 0. })
                                .group_hover("feature-menu-row", |style| style.opacity(1.))
                                .focus(|style| style.opacity(1.))
                                .tooltip(label.clone())
                                .accessibility_label(label)
                                .on_click(move |_, _, cx| {
                                    cx.stop_propagation();
                                    feature.toggle_pin(cx);
                                }),
                        )
                    })
            })
            .on_click(move |_, window, cx| {
                let _ = open_owner.update(cx, |shell, cx| open_feature.open(shell, window, cx));
            }),
        );
    }
    menu
}
