//! Plugin choices use the shared Kit search surface and contribution events.
use super::*;
use gpui_kit::component::{
    list::{ListDelegate, ListItem, ListState},
    *,
};
use gpui_kit::prelude::FluentBuilder as _;

struct Picker {
    state: Entity<ListState<Choices>>,
    _updates: Subscription,
    _enter: Subscription,
}

pub(super) fn open(registry: Entity<Registry>, key: Key, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    let picker = cx.new(|cx| {
        let state = cx.new(|cx| {
            ListState::new(
                Choices {
                    registry: registry.clone(),
                    key: key.clone(),
                    query: String::new(),
                    selected: None,
                },
                window,
                cx,
            )
            .searchable(true)
        });
        state.update(cx, |list, cx| {
            let selected = list.delegate().first(cx);
            list.set_selected_index(selected, window, cx);
        });
        let enter = crate::command_picker::intercept(&state, Choices::enabled, cx);
        let updates = cx.observe_in(&registry, window, |picker: &mut Picker, _, window, cx| {
            picker.state.update(cx, |list, cx| {
                let selected = list.delegate().first(cx);
                list.set_selected_index(selected, window, cx);
                cx.notify();
            });
            window.refresh();
            cx.notify();
        });
        Picker {
            state,
            _updates: updates,
            _enter: enter,
        }
    });
    let focus = picker.read(cx).state.clone();
    window.open_dialog(cx, move |dialog, window, cx| {
        let picker = picker.read(cx);
        let choices = picker.state.read(cx).delegate();
        let groups = choices.groups(cx);
        let height = crate::command_picker::extent(
            groups.iter().map(|(_, choices)| choices.len()).sum(),
            groups.iter().filter(|(label, _)| label.is_some()).count(),
            choices.height(cx),
            window,
        );
        crate::command_picker::dialog(
            dialog,
            "plugin-contribution-picker",
            &picker.state,
            crate::tr("connection_search"),
            window,
            cx,
        )
        .h(height)
    });
    window.defer(cx, move |window, cx| {
        focus.update(cx, |state, cx| state.focus(window, cx));
    });
    registry.update(cx, |registry, cx| {
        registry.invoke(&key, ui::EventKind::Search, "".into(), window, cx)
    });
}

struct Choices {
    registry: Entity<Registry>,
    key: Key,
    query: String,
    selected: Option<IndexPath>,
}

impl Choices {
    fn groups(&self, cx: &App) -> Vec<(Option<SharedString>, Vec<ui::Choice>)> {
        let Some(entry) = self.registry.read(cx).entry(&self.key, cx) else {
            return Vec::new();
        };
        let mut groups: Vec<(Option<SharedString>, Vec<ui::Choice>)> = Vec::new();
        for choice in entry.choices().iter().filter(|choice| {
            controls::label(&choice.label)
                .to_lowercase()
                .contains(&self.query)
        }) {
            let group = choice.group.as_ref().map(controls::label);
            if let Some((_, choices)) = groups.iter_mut().find(|(label, _)| *label == group) {
                choices.push(choice.clone());
            } else {
                groups.push((group, vec![choice.clone()]));
            }
        }
        groups
    }

    fn first(&self, cx: &App) -> Option<IndexPath> {
        self.enabled(cx).into_iter().next()
    }

    fn enabled(&self, cx: &App) -> Vec<IndexPath> {
        if !self
            .registry
            .read(cx)
            .entry(&self.key, cx)
            .is_some_and(|entry| entry.state.enabled)
        {
            return Vec::new();
        }
        self.groups(cx)
            .into_iter()
            .enumerate()
            .flat_map(|(section, (_, choices))| {
                choices
                    .into_iter()
                    .enumerate()
                    .filter(|(_, choice)| choice.enabled)
                    .map(move |(row, _)| IndexPath::new(row).section(section))
            })
            .collect()
    }

    fn height(&self, cx: &App) -> f32 {
        if self
            .registry
            .read(cx)
            .entry(&self.key, cx)
            .is_some_and(|entry| {
                entry
                    .choices()
                    .iter()
                    .any(|choice| choice.description.is_some())
            })
        {
            48.
        } else {
            crate::command_picker::ROW_HEIGHT
        }
    }
}

impl ListDelegate for Choices {
    type Item = ListItem;

    fn sections_count(&self, cx: &App) -> usize {
        self.groups(cx).len().max(1)
    }

    fn items_count(&self, section: usize, cx: &App) -> usize {
        self.groups(cx)
            .get(section)
            .map_or(0, |(_, choices)| choices.len())
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.to_lowercase();
        self.registry.update(cx, |registry, cx| {
            registry.invoke(&self.key, ui::EventKind::Search, query.into(), window, cx)
        });
        let selected = self.first(cx);
        let owner = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |list, cx| {
                list.set_selected_index(selected, window, cx);
                cx.notify();
            });
        });
        cx.notify();
        Task::ready(())
    }

    fn render_section_header(
        &mut self,
        section: usize,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<impl IntoElement> {
        let label = self.groups(cx).get(section)?.0.clone()?;
        Some(crate::command_picker::heading(label, cx))
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let entry = self.registry.read(cx).entry(&self.key, cx)?;
        let groups = self.groups(cx);
        let choice = groups.get(index.section)?.1.get(index.row)?.clone();
        let checked = entry.state.value.as_str() == Some(&choice.id);
        let disabled = !entry.state.enabled || !choice.enabled;
        Some(
            crate::command_picker::row(
                ("plugin-choice", index.section * 1000 + index.row),
                checked,
                disabled,
                cx,
            )
            .h(px(self.height(cx)))
            .when(!disabled, |item| {
                item.on_mouse_enter(cx.listener(move |list, _, window, cx| {
                    list.set_selected_index(Some(index), window, cx);
                    cx.notify();
                }))
            })
            .child(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap_2()
                    .children(choice.icon.as_ref().map(|icon| controls::icon(Some(icon))))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().truncate().child(controls::label(&choice.label)))
                            .when_some(choice.description.as_ref(), |column, description| {
                                column.child(
                                    div()
                                        .truncate()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(controls::label(description)),
                                )
                            }),
                    ),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_3()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(crate::tr(
                if self.registry.read(cx).entry(&self.key, cx).is_some() {
                    "connection_search_empty"
                } else {
                    "plugins_view_unavailable"
                },
            ))
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(entry) = self.registry.read(cx).entry(&self.key, cx) else {
            return;
        };
        let groups = self.groups(cx);
        let Some(choice) = self
            .selected
            .and_then(|index| groups.get(index.section)?.1.get(index.row))
            .filter(|choice| entry.state.enabled && choice.enabled)
        else {
            return;
        };
        self.registry.update(cx, |registry, cx| {
            registry.invoke(
                &self.key,
                ui::EventKind::Change,
                choice.id.clone().into(),
                window,
                cx,
            )
        });
        window.close_dialog(cx);
    }
}
