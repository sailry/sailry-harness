use gpui_kit::component::{
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
    *,
};
use gpui_kit::*;

use super::{
    Section, Workspace,
    group::{Group, Row},
};
use crate::tr;

impl Workspace {
    pub(super) fn preferences(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preferences = crate::preferences::data(cx);
        match self.section {
            Section::General => v_flex()
                .gap_8()
                .child(
                    Group::new("settings_general")
                        .heading(false)
                        .child(Row::new(
                            "settings_language",
                            div()
                                .w_48()
                                .debug_selector(|| "settings-language".into())
                                .child(
                                    Select::new(&self.language)
                                        .accessibility_label(tr("settings_language")),
                                ),
                        )),
                )
                .child(
                    Group::new("settings_notifications")
                        .children(
                            [
                                "settings_notify_system",
                                "settings_notify_approval",
                                "settings_notify_completed",
                                "settings_notify_failed",
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(index, key)| {
                                Row::new(
                                    key,
                                    div().debug_selector(move || format!("toggle-{key}")).child(
                                        Switch::new(key)
                                            .accessibility_label(tr(key))
                                            .checked(preferences.notifications[index])
                                            .on_click(cx.listener(move |_, checked, _, cx| {
                                                crate::preferences::update(cx, |data| {
                                                    data.notifications[index] = *checked
                                                });
                                            })),
                                    ),
                                )
                                .description(&format!("{key}_description"))
                            }),
                        )
                        .child(
                            Row::new(
                                "settings_toast_duration",
                                div()
                                    .w_24()
                                    .debug_selector(|| "toast-duration".into())
                                    .child(
                                        Select::new(&self.toast_duration)
                                            .accessibility_label(tr("settings_toast_duration")),
                                    ),
                            )
                            .description("settings_toast_description"),
                        ),
                )
                .into_any_element(),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone)]
pub(super) struct Language(crate::locale::Language);

impl SearchableListItem for Language {
    type Value = crate::locale::Language;
    fn title(&self) -> SharedString {
        tr(self.0.label())
    }
    fn value(&self) -> &Self::Value {
        &self.0
    }
}

pub(super) fn language(
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<SelectState<Vec<Language>>> {
    let selected = crate::preferences::data(cx).language.unwrap_or_default();
    let values = crate::locale::Language::ALL;
    let state = cx.new(|cx| {
        SelectState::new(
            values.into_iter().map(Language).collect::<Vec<_>>(),
            values
                .iter()
                .position(|value| *value == selected)
                .map(IndexPath::new),
            window,
            cx,
        )
    });
    cx.subscribe_in(&state, window, |_, _, event, window, cx| {
        if let SelectEvent::Confirm(Some(value)) = event {
            if crate::preferences::data(cx).language.unwrap_or_default() == *value {
                return;
            }
            crate::preferences::update(cx, |data| data.language = Some(*value));
            if cx
                .global::<crate::preferences::Preferences>()
                .error
                .is_none()
            {
                let message = tr("settings_language_restart");
                crate::feedback::toast(
                    window,
                    message.clone(),
                    notification::Notification::info(message),
                    cx,
                );
            }
        }
    })
    .detach();
    state
}

fn seconds(value: u64) -> SharedString {
    rust_i18n::t!("settings_seconds", count = value)
        .to_string()
        .into()
}

#[derive(Clone)]
pub(super) struct Duration(u64);
impl SearchableListItem for Duration {
    type Value = u64;
    fn title(&self) -> SharedString {
        seconds(self.0)
    }
    fn value(&self) -> &u64 {
        &self.0
    }
}

pub(super) fn duration(
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Entity<SelectState<Vec<Duration>>> {
    let selected = crate::preferences::data(cx).toast_seconds;
    let values = [3, 5, 8, 10];
    let state = cx.new(|cx| {
        SelectState::new(
            values.into_iter().map(Duration).collect::<Vec<_>>(),
            values
                .iter()
                .position(|value| *value == selected)
                .map(IndexPath::new),
            window,
            cx,
        )
    });
    cx.subscribe(&state, |_, _, event, cx| {
        if let SelectEvent::Confirm(Some(value)) = event {
            crate::preferences::update(cx, |data| data.toast_seconds = *value);
        }
    })
    .detach();
    state
}
