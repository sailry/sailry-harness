//! Standard Kit search owns focus, filtering, keyboard selection and dialog dismissal.
use super::*;
use gpui_kit::component::{
    command::{Command, CommandGroup, CommandItem, CommandState},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;

struct Picker {
    registry: Entity<Registry>,
    key: Key,
    state: Entity<CommandState>,
    _updates: Subscription,
}

pub(super) fn open(registry: Entity<Registry>, key: Key, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let picker = cx.new(|cx| {
        let updates = cx.observe_in(&registry, window, |_, _, window, cx| {
            window.refresh();
            cx.notify();
        });
        Picker {
            registry: registry.clone(),
            key: key.clone(),
            state: cx.new(|cx| CommandState::new(window, cx)),
            _updates: updates,
        }
    });
    let focus = picker.read(cx).state.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let picker = picker.read(cx);
        let entry = picker.registry.read(cx).entry(&picker.key, cx);
        let Some(entry) = entry else {
            return dialog.child(crate::tr("plugins_view_unavailable"));
        };
        let mut groups: Vec<(Option<SharedString>, Vec<ui::Choice>)> = Vec::new();
        for choice in entry.choices() {
            let group = choice.group.as_ref().map(controls::label);
            if let Some((_, items)) = groups.iter_mut().find(|(label, _)| *label == group) {
                items.push(choice.clone());
            } else {
                groups.push((group, vec![choice.clone()]));
            }
        }
        let mut command = Command::new(&picker.state)
            .bordered(false)
            .text_sm()
            .max_h(crate::command_picker::body_height(window))
            .placeholder(crate::tr("connection_search"));
        for (label, choices) in &groups {
            let mut group = CommandGroup::new();
            if let Some(label) = label {
                group = group.label(label.clone());
            }
            for choice in choices {
                let mut item = CommandItem::new()
                    .label(controls::label(&choice.label))
                    .disabled(!entry.state.enabled || !choice.enabled)
                    .checked(entry.state.value.as_str() == Some(&choice.id));
                if let Some(icon) = &choice.icon {
                    item = item.icon(controls::icon(Some(icon)));
                }
                if let Some(description) = &choice.description {
                    let title = controls::label(&choice.label);
                    let description = controls::label(description);
                    let icon = choice.icon.clone();
                    let checked = entry.state.value.as_str() == Some(&choice.id);
                    item = item.child(move |_, cx| {
                        h_flex()
                            .w_full()
                            .gap_2()
                            .when_some(icon.clone(), |row, icon| {
                                row.child(controls::icon(Some(&icon)))
                            })
                            .child(
                                v_flex().flex_1().gap_0p5().child(title.clone()).child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(description.clone()),
                                ),
                            )
                            .when(checked, |row| row.child(IconName::Check))
                    });
                }
                group = group.item(item);
            }
            command = command.group(group);
        }
        let owner = picker.registry.downgrade();
        let query_owner = owner.clone();
        let key = picker.key.clone();
        let query_key = key.clone();
        crate::command_picker::frame(
            dialog,
            "plugin-contribution-picker",
            command
                .on_query(move |query, window, cx| {
                    let _ = query_owner.update(cx, |registry, cx| {
                        registry.invoke(&query_key, ui::EventKind::Search, query.into(), window, cx)
                    });
                })
                .on_confirm(move |index, window, cx| {
                    if let Some(choice) = groups
                        .get(index.section)
                        .and_then(|(_, choices)| choices.get(index.row))
                        .filter(|choice| entry.state.enabled && choice.enabled)
                    {
                        let _ = owner.update(cx, |registry, cx| {
                            registry.invoke(
                                &key,
                                ui::EventKind::Change,
                                choice.id.clone().into(),
                                window,
                                cx,
                            )
                        });
                        window.close_dialog(cx);
                    }
                }),
            window,
        )
    });
    window.defer(cx, move |window, cx| {
        focus.update(cx, |state, cx| state.focus(window, cx));
    });
    registry.update(cx, |registry, cx| {
        registry.invoke(&key, ui::EventKind::Search, "".into(), window, cx)
    });
}
