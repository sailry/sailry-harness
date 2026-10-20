use super::*;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{
    accordion::Accordion,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    radio::Radio,
    *,
};

impl Picker {
    fn row(&self, channel: &Group, model: &Choice, cx: &mut Context<Self>) -> AnyElement {
        let id = channel.id;
        let model_id = model.id.clone();
        let selection = Selection {
            channel: id,
            model: model_id.clone(),
        };
        let selected = self.selected.as_ref() == Some(&selection);
        let effort = if selected {
            self.current_effort.unwrap_or(model.effort)
        } else {
            model.effort
        };
        let radio_selection = selection.clone();
        let owner = cx.entity().downgrade();
        let choices = model.efforts.clone();
        h_flex()
            .id((SharedString::from(model.id.clone()), id))
            .w_full()
            .min_w_0()
            .gap_2()
            .pr_3()
            .rounded(cx.theme().radius_lg)
            .child(
                Button::new((SharedString::from(format!("model-{}", model.id)), id))
                    .text()
                    .flex_1()
                    .min_w_0()
                    .h_8()
                    .px_2()
                    .disabled(self.disabled)
                    .accessibility_label(model.label.clone())
                    .debug_selector(move || format!("composer-model-option-{id}-{model_id}"))
                    .child(
                        h_flex().w_full().min_w_0().gap_2().child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .line_height(px(20.))
                                .child(model.label.clone()),
                        ),
                    )
                    .on_click(cx.listener(move |picker, _, window, cx| {
                        picker.pick(selection.clone(), effort, window, cx)
                    })),
            )
            .when(
                selected && self.current_effort.is_some() && !choices.is_empty(),
                |row| {
                    let label = crate::reasoning::label(effort);
                    let selector = label.clone();
                    row.child(
                        Button::new("composer-model-effort")
                            .custom(
                                crate::theme::subtle_button(cx)
                                    .color(cx.theme().secondary_active)
                                    .hover(cx.theme().secondary_active)
                                    .active(cx.theme().secondary_active),
                            )
                            .small()
                            .h_7()
                            .px_3()
                            .rounded_full()
                            .accessibility_label(label.clone())
                            .child(
                                div()
                                    .debug_selector(move || {
                                        format!("composer-model-effort-value-{selector}")
                                    })
                                    .text_sm()
                                    .line_height(px(20.))
                                    .child(label),
                            )
                            .dropdown_caret(true)
                            .disabled(self.disabled)
                            .debug_selector(|| "composer-model-effort".into())
                            .dropdown_menu(move |menu, _, _| {
                                choices.iter().fold(menu, |menu, choice| {
                                    let choice = *choice;
                                    let owner = owner.clone();
                                    menu.item(
                                        PopupMenuItem::new(crate::reasoning::label(choice))
                                            .checked(choice == effort)
                                            .on_click(move |_, _, cx| {
                                                _ = owner.update(cx, |picker, cx| {
                                                    if !picker.disabled {
                                                        cx.emit(EffortPicked(choice));
                                                    }
                                                });
                                            }),
                                    )
                                })
                            }),
                    )
                },
            )
            .child(
                Radio::new((SharedString::from(format!("model-radio-{}", model.id)), id))
                    .checked(selected)
                    .disabled(self.disabled)
                    .small()
                    .w_4()
                    .justify_center()
                    .accessibility_label(model.label.clone())
                    .on_click(cx.listener(move |picker, _, window, cx| {
                        picker.pick(radio_selection.clone(), effort, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut seen = BTreeSet::new();
        let families = ModelCategory::ALL
            .into_iter()
            .flat_map(|category| {
                self.channels.iter().filter(move |channel| {
                    channel.category == category && !channel.models.is_empty()
                })
            })
            .filter(|channel| seen.insert(channel.family.clone()))
            .collect::<Vec<_>>();
        v_flex()
            .id("model-family-scroll")
            .overflow_y_scrollbar()
            .w_12()
            .min_h_0()
            .flex_shrink_0()
            .p_1()
            .gap_1()
            .rounded(cx.theme().radius_2xl())
            .bg(cx.theme().secondary)
            .when(self.current_effort.is_some(), |rail| {
                rail.child(
                    div().debug_selector(|| "model-controls-back".into()).child(
                        Button::new("model-controls-back")
                            .text()
                            .icon(IconName::ArrowLeft)
                            .tooltip(tr("composer_model_settings"))
                            .w_10()
                            .h_10()
                            .rounded_full()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Back))),
                    ),
                )
            })
            .children(families.into_iter().enumerate().map(|(index, channel)| {
                let family = Some(channel.family.clone());
                let selected = self.family == family;
                let label = channel.directory.as_ref().map_or_else(
                    || tr(channel.category.key()),
                    |entry| crate::settings::type_label(entry).into(),
                );
                let icon = channel.directory.as_ref().map_or_else(
                    || channel.category.icon().size_4().into_any_element(),
                    |entry| crate::settings::type_icon(entry, rems(1.)),
                );

                div()
                    .debug_selector(move || format!("composer-model-family-{index}"))
                    .child(
                        Button::new(("model-family", index))
                            .text()
                            .child(icon)
                            .tooltip(label)
                            .selected(selected)
                            .when(selected, |button| button.bg(cx.theme().popover))
                            .text_color(if selected {
                                cx.theme().foreground
                            } else {
                                cx.theme().muted_foreground
                            })
                            .w_10()
                            .h_10()
                            .rounded_full()
                            .on_click(cx.listener(move |picker, _, _, cx| {
                                picker.family = family.clone();
                                cx.notify();
                            })),
                    )
            }))
            .map(|rail| {
                div()
                    .w_12()
                    .h_full()
                    .min_h_0()
                    .flex_shrink_0()
                    .debug_selector(|| "composer-model-navigation".into())
                    .child(rail)
            })
            .into_any_element()
    }
}

impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let channels: Vec<_> = self
            .channels
            .iter()
            .filter(|channel| {
                self.family
                    .as_ref()
                    .is_none_or(|family| *family == channel.family)
            })
            .filter(|channel| !channel.models.is_empty())
            .cloned()
            .collect();
        let visible_ids: Vec<_> = channels.iter().map(|c| c.id).collect();
        let mut groups = Accordion::new("composer-model-groups")
            .multiple(true)
            .bordered(false)
            .h_auto()
            .flex_shrink_0();
        for channel in &channels {
            let id = channel.id;
            let rows = channel
                .models
                .iter()
                .map(|model| self.row(channel, model, cx))
                .collect::<Vec<_>>();
            groups = groups.item(|item| {
                item.title(
                    div()
                        .id(SharedString::from(format!("composer-model-group-{id}")))
                        .test_support()
                        .debug_selector(move || format!("composer-model-group-{id}"))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(channel.name.clone()),
                )
                .bg(transparent_black())
                .open(!self.collapsed.contains(&id))
                .title_style(StyleRefinement::default().pl_2().pr_3p5().py_1p5())
                .content_style(StyleRefinement::default().px_0().pb_0())
                .child(v_flex().w_full().children(rows))
            });
        }
        h_flex()
            .debug_selector(|| "composer-model-picker".into())
            .flex_shrink_0()
            .w(panel_size(window).width)
            .when(self.categories, |body| body.h(panel_size(window).height))
            .items_stretch()
            .gap_3()
            .when(self.categories, |body| body.child(self.navigation(cx)))
            .child(
                v_flex().relative().flex_1().min_w_0().gap_2().child(
                    div()
                        .id("composer-model-scroll")
                        .test_support()
                        .debug_selector(|| "composer-model-scroll".into())
                        .relative()
                        .max_h(panel_size(window).height)
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .when(channels.is_empty(), |body| {
                            body.child(
                                div()
                                    .debug_selector(|| "composer-model-empty".into())
                                    .p_2()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("composer_model_empty")),
                            )
                        })
                        .when(!channels.is_empty(), |body| {
                            body.child(groups.on_toggle_click(cx.listener(
                                move |this, open: &[usize], _, cx| {
                                    for (index, id) in visible_ids.iter().enumerate() {
                                        if open.contains(&index) {
                                            this.collapsed.remove(id);
                                        } else {
                                            this.collapsed.insert(*id);
                                        }
                                    }
                                    cx.notify();
                                },
                            )))
                        }),
                ),
            )
    }
}
