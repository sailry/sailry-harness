use super::*;

impl Render for Panel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = preferences::data(cx).dictation.unwrap_or_default();
        let selected = options.microphone.clone();
        let devices = self.microphones.clone();
        let microphone = selected
            .as_ref()
            .map(|id| {
                devices
                    .iter()
                    .find(|(key, _)| key == id)
                    .map(|(_, label)| label.clone())
                    .unwrap_or_else(|| {
                        tr(
                            if matches!(self.state, State::Unavailable | State::Checking) {
                                "dictation_device_loading"
                            } else {
                                "dictation_device_missing"
                            },
                        )
                        .to_string()
                    })
            })
            .unwrap_or_else(|| tr("dictation_microphone_default").to_string());
        let models = Group::new("dictation_local_model")
            .action(
                Button::new("speech-model-library")
                    .label(tr("model_library_manage"))
                    .debug_selector(|| "speech-model-library".into())
                    .disabled(self.binding.is_none())
                    .on_click(cx.listener(|panel, _, window, cx| panel.library(window, cx))),
            )
            .children(self.model_rows(cx.entity(), false, cx));
        v_flex()
            .gap_6()
            .child(
                Group::new("dictation_input")
                    .child(
                        Row::new(
                            "dictation_access",
                            h_flex().when(permission::settings_url().is_some(), |row| {
                                if self.permission == Status::Granted {
                                    return row.child(
                                        div()
                                            .text_color(cx.theme().muted_foreground)
                                            .debug_selector(|| "dictation-access-Granted".into())
                                            .child(tr("dictation_access_granted")),
                                    );
                                }
                                row.child(
                                    Button::new("dictation-authorize")
                                        .label(tr("permission_request"))
                                        .debug_selector(|| "dictation-authorize".into())
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.authorize(window, cx)
                                        })),
                                )
                            }),
                        )
                        .description("dictation_access_hint")
                        .wide(),
                    )
                    .child(
                        Row::new(
                            "dictation_microphone_label",
                            Button::new("dictation-microphone")
                                .w_full()
                                .label(microphone)
                                .dropdown_caret(true)
                                .debug_selector(|| "dictation-microphone".into())
                                .dropdown_menu(move |menu, _, _| {
                                    std::iter::once((
                                        None,
                                        tr("dictation_microphone_default").to_string(),
                                    ))
                                    .chain(
                                        devices
                                            .iter()
                                            .map(|(id, name)| (Some(id.clone()), name.clone())),
                                    )
                                    .fold(
                                        menu,
                                        |menu, (id, name)| {
                                            menu.item(
                                                PopupMenuItem::new(name)
                                                    .checked(id == selected)
                                                    .on_click(move |_, _, cx| {
                                                        preferences::update(cx, |data| {
                                                            data.dictation
                                                                .get_or_insert_default()
                                                                .microphone = id.clone();
                                                        })
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        )
                        .wide(),
                    )
                    .child(
                        Row::new(
                            "dictation_language",
                            Button::new("dictation-language")
                                .w_full()
                                .label(tr(language_key(options.language)))
                                .dropdown_caret(true)
                                .debug_selector(|| "dictation-language".into())
                                .dropdown_menu(move |menu, _, _| {
                                    preferences::Language::ALL.into_iter().fold(
                                        menu,
                                        |menu, language| {
                                            menu.item(
                                                PopupMenuItem::new(tr(language_key(language)))
                                                    .checked(language == options.language)
                                                    .on_click(move |_, _, cx| {
                                                        preferences::update(cx, |data| {
                                                            data.dictation
                                                                .get_or_insert_default()
                                                                .language = language;
                                                        })
                                                    }),
                                            )
                                        },
                                    )
                                }),
                        )
                        .wide(),
                    ),
            )
            .child(models)
    }
}
