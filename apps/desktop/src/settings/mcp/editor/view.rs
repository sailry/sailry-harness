use super::*;
use crate::theme::DialogStyle as _;
use gpui_kit::component::{
    dialog::DialogFooter,
    form::Field,
    tab::{Tab, TabBar},
};

pub(super) fn open(editor: Entity<Editor>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let save = editor.clone();
        let cancel = editor.clone();
        let close = editor.clone();
        let details = editor.clone();
        let reload = editor.clone();
        let state = editor.read(cx);
        dialog
            .form_title(tr(if state.original.is_some() {
                "mcp_edit"
            } else {
                "mcp_add"
            }))
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .max_h(window.viewport_size().height * 0.85)
            .overlay_closable(false)
            .on_close(move |_, window, cx| close.update(cx, |editor, cx| editor.close(window, cx)))
            .child(editor.clone())
            .footer(
                v_flex()
                    .w_full()
                    .gap_2()
                    .when(state.details, |view| {
                        view.children(
                            state.detail.clone().map(|text| {
                                div().text_xs().max_h_16().overflow_hidden().child(text)
                            }),
                        )
                    })
                    .child(
                        DialogFooter::new()
                            .w_full()
                            .gap_2()
                            .flex_wrap()
                            .when_some(
                                state.original.as_ref().filter(|info| {
                                    info.mcp_source.as_ref().is_some_and(|source| {
                                        source.transport() != McpTransport::Stdio
                                    })
                                }),
                                |row, info| {
                                    let binding = state.binding.clone();
                                    let package = info.summary.reference();
                                    row.child(
                                        Button::new("mcp-authorize")
                                            .ghost()
                                            .label(tr("mcp_oauth_title"))
                                            .debug_selector(|| "mcp-authorize".into())
                                            .disabled(state.locked())
                                            .on_click(move |_, window, cx| {
                                                plugins::authorization::open(
                                                    binding.clone(),
                                                    package.clone(),
                                                    "server".into(),
                                                    window,
                                                    cx,
                                                );
                                            }),
                                    )
                                },
                            )
                            .when(!state.loaded, |row| {
                                row.child(
                                    Button::new("mcp-reload")
                                        .label(tr("plugins_settings_reload"))
                                        .disabled(state.loading)
                                        .on_click(move |_, window, cx| {
                                            reload.update(cx, |editor, cx| editor.load(window, cx))
                                        }),
                                )
                            })
                            .when(state.detail.is_some(), |row| {
                                row.child(
                                    Button::new("mcp-diagnostics")
                                        .ghost()
                                        .label(tr("plugins_settings_diagnostics"))
                                        .on_click(move |_, _, cx| {
                                            details.update(cx, |editor, cx| {
                                                editor.details = !editor.details;
                                                cx.notify();
                                            })
                                        }),
                                )
                            })
                            .child(
                                Button::new("mcp-cancel")
                                    .debug_selector(|| "mcp-cancel".into())
                                    .label(tr("settings_cancel"))
                                    .on_click(move |_, window, cx| {
                                        cancel.update(cx, |editor, cx| editor.close(window, cx));
                                        window.close_dialog(cx);
                                    }),
                            )
                            .child(
                                Button::new("mcp-save")
                                    .primary()
                                    .debug_selector(|| "mcp-save".into())
                                    .loading(state.pending)
                                    .disabled(state.pending || state.loading || !state.loaded)
                                    .label(tr(if state.request.is_some() {
                                        "plugins_retry"
                                    } else {
                                        "settings_save"
                                    }))
                                    .on_click(move |_, window, cx| {
                                        save.update(cx, |editor, cx| editor.save(window, cx))
                                    }),
                            ),
                    ),
            )
    });
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locked = self.locked();
        let same_kind = self.same_kind();
        let transports = [
            McpTransport::Stdio,
            McpTransport::StreamableHttp,
            McpTransport::Sse,
        ];
        v_flex()
            .id("mcp-editor")
            .debug_selector(|| "mcp-editor".into())
            .gap_3()
            .child(
                Field::new().label(tr("settings_name")).child(
                    div().debug_selector(|| "mcp-name".into()).child(
                        Input::new(&self.name)
                            .disabled(locked || self.original.is_some())
                            .aria_label(tr("settings_name")),
                    ),
                ),
            )
            .child(
                Field::new().label(tr("mcp_transport")).child(
                    TabBar::new("mcp-transport")
                        .segmented()
                        .equal_width()
                        .w_full()
                        .selected_index(
                            transports
                                .iter()
                                .position(|transport| *transport == self.transport)
                                .unwrap_or(0),
                        )
                        .children(
                            transports
                                .into_iter()
                                .enumerate()
                                .map(|(index, transport)| {
                                    Tab::new()
                                        .label(tr(transport_key(transport)))
                                        .disabled(locked)
                                        .debug_selector(move || format!("mcp-transport-{index}"))
                                }),
                        )
                        .on_click(cx.listener(move |editor, index, _, cx| {
                            editor.transport = transports[*index];
                            cx.notify();
                        })),
                ),
            )
            .when(self.transport == McpTransport::Stdio, |view| {
                view.child(
                    Field::new().label(tr("mcp_command")).child(
                        div().debug_selector(|| "mcp-command".into()).child(
                            Input::new(&self.command)
                                .disabled(locked)
                                .aria_label(tr("mcp_command")),
                        ),
                    ),
                )
                .child(
                    Field::new().label(tr("mcp_arguments")).child(
                        v_flex()
                            .gap_2()
                            .children(self.args.iter().enumerate().map(|(index, arg)| {
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .debug_selector(move || format!("mcp-argument-{index}"))
                                            .child(
                                                Input::new(arg)
                                                    .disabled(locked)
                                                    .aria_label(tr("mcp_argument")),
                                            ),
                                    )
                                    .child(
                                        Button::new(("mcp-remove-argument", index))
                                            .ghost()
                                            .icon(IconName::CircleX)
                                            .disabled(locked)
                                            .accessibility_label(tr("mcp_remove_argument"))
                                            .on_click(cx.listener(move |editor, _, _, cx| {
                                                editor.args.remove(index);
                                                cx.notify();
                                            })),
                                    )
                            }))
                            .child(
                                Button::new("mcp-add-argument")
                                    .debug_selector(|| "mcp-add-argument".into())
                                    .disabled(locked)
                                    .label(tr("mcp_add_argument"))
                                    .on_click(cx.listener(|editor, _, window, cx| {
                                        editor.args.push(text_input(
                                            String::new(),
                                            "mcp_argument",
                                            window,
                                            cx,
                                        ));
                                        cx.notify();
                                    })),
                            ),
                    ),
                )
            })
            .when(self.transport != McpTransport::Stdio, |view| {
                view.child(
                    Field::new().label(tr("mcp_url")).child(
                        div().debug_selector(|| "mcp-url".into()).child(
                            Input::new(&self.url)
                                .disabled(locked)
                                .aria_label(tr("mcp_url")),
                        ),
                    ),
                )
            })
            .child(
                Field::new()
                    .label(tr(if self.transport == McpTransport::Stdio {
                        "mcp_environment"
                    } else {
                        "mcp_headers"
                    }))
                    .child(
                        v_flex()
                            .gap_2()
                            .children(self.slots.iter().map(|slot| {
                                let id = slot.id;
                                GroupBox::new()
                                    .id(("mcp-slot", id))
                                    .fill()
                                    .content_style(StyleRefinement::default().p_3())
                                    .child(
                                        v_flex()
                                            .gap_2()
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .debug_selector(move || {
                                                                format!("mcp-slot-name-{id}")
                                                            })
                                                            .child(
                                                                Input::new(&slot.name)
                                                                    .disabled(locked)
                                                                    .aria_label(tr(
                                                                        "mcp_slot_name",
                                                                    )),
                                                            ),
                                                    )
                                                    .child(
                                                        Button::new(("mcp-remove-slot", id))
                                                            .ghost()
                                                            .icon(IconName::CircleX)
                                                            .disabled(locked)
                                                            .accessibility_label(tr(
                                                                "mcp_remove_slot",
                                                            ))
                                                            .debug_selector(move || {
                                                                format!("mcp-remove-slot-{id}")
                                                            })
                                                            .on_click(cx.listener(
                                                                move |editor, _, window, cx| {
                                                                    if let Some(index) = editor
                                                                        .slots
                                                                        .iter()
                                                                        .position(|slot| {
                                                                            slot.id == id
                                                                        })
                                                                    {
                                                                        editor
                                                                            .slots
                                                                            .remove(index)
                                                                            .clear_value(
                                                                                window, cx,
                                                                            );
                                                                        cx.notify();
                                                                    }
                                                                },
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .debug_selector(move || {
                                                        format!("mcp-slot-value-{id}")
                                                    })
                                                    .child(
                                                        Input::new(&slot.value)
                                                            .disabled(locked)
                                                            .mask_toggle()
                                                            .aria_label(tr("mcp_slot_value")),
                                                    ),
                                            )
                                            .when(
                                                slot.configured
                                                    && slot.value.read(cx).value().is_empty()
                                                    && slot.original.as_deref()
                                                        == Some(slot.name.read(cx).value().trim()),
                                                |view| {
                                                    view.child(
                                                        h_flex()
                                                            .items_center()
                                                            .gap_2()
                                                            .child(
                                                                div()
                                                                    .flex_1()
                                                                    .text_xs()
                                                                    .text_color(
                                                                        cx.theme().muted_foreground,
                                                                    )
                                                                    .child(tr(
                                                                        if slot.clear || !same_kind
                                                                        {
                                                                            "mcp_secret_cleared"
                                                                        } else {
                                                                            "mcp_secret_saved"
                                                                        },
                                                                    )),
                                                            )
                                                            .when(same_kind, |row| {
                                                                row.child(
                                                            Button::new(("mcp-clear-slot", id))
                                                                .ghost()
                                                                .small()
                                                                .disabled(locked)
                                                                .label(tr(if slot.clear {
                                                                    "mcp_keep_secret"
                                                                } else {
                                                                    "mcp_clear_secret"
                                                                }))
                                                                .debug_selector(move || {
                                                                    format!("mcp-clear-slot-{id}")
                                                                })
                                                                .on_click(cx.listener(
                                                                    move |editor, _, window, cx| {
                                                                        if let Some(slot) = editor
                                                                            .slots
                                                                            .iter_mut()
                                                                            .find(|slot| {
                                                                                slot.id == id
                                                                            })
                                                                        {
                                                                            slot.clear =
                                                                                !slot.clear;
                                                                            slot.clear_value(
                                                                                window, cx,
                                                                            );
                                                                            cx.notify();
                                                                        }
                                                                    },
                                                                )),
                                                        )
                                                            }),
                                                    )
                                                },
                                            ),
                                    )
                            }))
                            .child(
                                Button::new("mcp-add-slot")
                                    .debug_selector(|| "mcp-add-slot".into())
                                    .disabled(
                                        locked
                                            || self.slots.len()
                                                >= sailry_protocol::plugin::settings::MAX_FIELDS,
                                    )
                                    .label(tr(if self.transport == McpTransport::Stdio {
                                        "mcp_add_environment"
                                    } else {
                                        "mcp_add_header"
                                    }))
                                    .on_click(cx.listener(|editor, _, window, cx| {
                                        editor.add_slot(window, cx)
                                    })),
                            ),
                    ),
            )
            .when(self.loading, |view| view.child(spinner::Spinner::new()))
    }
}
