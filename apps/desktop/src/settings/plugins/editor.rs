use super::{Plugin, Workspace};
use crate::theme::DialogStyle as _;
use crate::tr;
use gpui_kit::component::{
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    form::Field,
    input::{Input, InputState},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

struct Editor {
    owner: Entity<Workspace>,
    index: Option<usize>,
    package: Entity<InputState>,
    directory: Entity<InputState>,
    grants: [bool; 2],
    update: bool,
    error: bool,
}

pub(super) fn open(
    owner: Entity<Workspace>,
    index: Option<usize>,
    update: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let initial = index
        .map(|index| owner.read(cx).plugins[index].clone())
        .unwrap_or_else(Plugin::example);
    let editor = cx.new(|cx| Editor {
        owner,
        index,
        update,
        grants: initial.grants,
        error: false,
        package: cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial.package)
                .placeholder(tr("form_package_hint"))
        }),
        directory: cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(initial.directory)
                .placeholder(tr("form_directory_hint"))
        }),
    });
    editor.update(cx, |_, cx| {
        crate::feedback::observe(window, cx, |view, _| {
            view.error
                .then_some("plugins_package_required")
                .into_iter()
                .collect()
        });
    });
    window.open_dialog(cx, move |dialog, window, _| {
        let save = editor.clone();
        dialog
            .form_title(tr(if !update {
                "plugins_configure"
            } else if index.is_some() {
                "plugins_update"
            } else {
                "plugins_install"
            }))
            .w((window.viewport_size().width - px(48.)).min(px(620.)))
            .child(editor.clone())
            .footer(
                gpui_kit::component::dialog::DialogFooter::new()
                    .w_full()
                    .gap_2()
                    .child(
                        Button::new("plugin-cancel")
                            .label(tr("settings_cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("plugin-save")
                            .primary()
                            .label(tr("settings_save_preview"))
                            .on_click(move |_, window, cx| {
                                save.update(cx, |editor, cx| {
                                    let package = editor.package.read(cx).value().trim().to_owned();
                                    if package.is_empty() {
                                        editor.error = true;
                                        cx.notify();
                                        return;
                                    }
                                    let directory = editor.directory.read(cx).value().to_string();
                                    editor.owner.update(cx, |this, cx| {
                                        if let Some(index) = editor.index {
                                            let plugin = &mut this.plugins[index];
                                            plugin.package = package;
                                            plugin.directory = directory;
                                            plugin.grants = editor.grants;
                                            if editor.update {
                                                plugin.revision += 1;
                                            }
                                        } else {
                                            let mut plugin = Plugin::example();
                                            this.plugin_serial += 1;
                                            plugin.id =
                                                format!("preview.toolkit.{}", this.plugin_serial);
                                            plugin.package = package;
                                            plugin.directory = directory;
                                            plugin.grants = editor.grants;
                                            this.plugins.push(plugin);
                                        }
                                        cx.notify();
                                    });
                                    window.close_dialog(cx);
                                })
                            }),
                    ),
            )
    });
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("plugin-editor")
            .debug_selector(|| "plugin-editor".into())
            .max_h((window.viewport_size().height - px(230.)).max(px(160.)))
            .overflow_y_scrollbar()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(tr("plugins_install_preview")),
            )
            .when(self.update, |this| {
                this.child(
                    gpui_kit::component::form::Form::vertical().child(
                        Field::new()
                            .label(tr("plugins_package"))
                            .child(Input::new(&self.package).aria_label(tr("plugins_package"))),
                    ),
                )
            })
            .child(
                gpui_kit::component::form::Form::vertical().child(
                    Field::new()
                        .label(tr("plugins_directory"))
                        .child(Input::new(&self.directory).aria_label(tr("plugins_directory"))),
                ),
            )
            .child(
                gpui_kit::component::form::Form::vertical().child(
                    Field::new().label(tr("plugins_capabilities")).child(
                        v_flex().gap_2().children(
                            ["plugins_cap_files", "plugins_cap_network"]
                                .into_iter()
                                .enumerate()
                                .map(|(index, key)| {
                                    Checkbox::new(key)
                                        .text_sm()
                                        .label(tr(key))
                                        .checked(self.grants[index])
                                        .on_click(cx.listener(move |this, checked, _, cx| {
                                            this.grants[index] = *checked;
                                            cx.notify();
                                        }))
                                }),
                        ),
                    ),
                ),
            )
    }
}
