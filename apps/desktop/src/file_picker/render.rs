use super::*;
mod grid;
mod list;
mod sidebar;
mod toolbar;
use gpui_kit::component::{dialog::DialogFooter, table::DataTable, tooltip::Tooltip};
pub(super) use list::Rows;
impl Render for Picker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = (window.viewport_size().width - px(48.)).min(px(900.)) - px(48.);
        let height = (window.viewport_size().height - px(300.)).clamp(px(120.), px(480.));
        let content = width - px(188.);
        let inset = window.rem_size() * 1.5;
        let gap = window.rem_size() * 0.5;
        let grid_width = (content - inset - px(2.)).max(px(1.));
        self.columns = if self.grid {
            (f32::from(grid_width + gap) / f32::from(window.rem_size() * 6.75 + gap))
                .floor()
                .clamp(1., 8.) as usize
        } else {
            1
        };
        self.table.update(cx, |table, cx| {
            let width = (f32::from(content - inset) - 192.).max(80.);
            if table.delegate().width != width {
                table.delegate_mut().width = width;
                table.refresh(cx);
            }
        });
        v_flex()
            .id("directory-picker")
            .debug_selector(|| "directory-picker".into())
            .gap_3()
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(Self::dispatch))
            .on_action(cx.listener(
                |picker, _: &gpui_kit::component::dialog::Confirm, window, cx| {
                    if picker.focus.is_focused(window)
                        && let Some(index) = picker.focused
                    {
                        picker.activate(index, window, cx);
                    }
                    cx.stop_propagation();
                },
            ))
            .when(self.client.is_none(), |body| {
                body.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(tr("directory_preview")),
                )
            })
            .child(self.toolbar(window, cx))
            .child(
                h_flex()
                    .h(height)
                    .items_stretch()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .child(self.sidebar(cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(
                                div()
                                    .id("directory-content")
                                    .debug_selector(|| "directory-list".into())
                                    .relative()
                                    .p_3()
                                    .track_focus(&self.focus)
                                    .flex_1()
                                    .min_h_0()
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(|picker, event, window, cx| {
                                            picker.menu(None, event, window, cx)
                                        }),
                                    )
                                    .when(
                                        self.listing.is_some()
                                            && (!self.visible.is_empty() || !self.grid),
                                        |body| {
                                            body.child(if self.grid {
                                                self.grid(grid_width, gap, window.rem_size(), cx)
                                            } else {
                                                DataTable::new(&self.table)
                                                    .bordered(false)
                                                    .stripe(true)
                                                    .with_size(gpui_kit::component::Size::Size(px(
                                                        28.,
                                                    )))
                                                    .into_any_element()
                                            })
                                        },
                                    )
                                    .when(
                                        self.listing.is_some()
                                            && self.grid
                                            && self.visible.is_empty(),
                                        |body| {
                                            body.child(
                                                div()
                                                    .debug_selector(|| "directory-empty".into())
                                                    .size_full()
                                                    .child(crate::empty_state::panel(
                                                        IconName::FolderOpen,
                                                        if self.filter.read(cx).value().is_empty() {
                                                            "files_directory_empty"
                                                        } else {
                                                            "directory_no_matches"
                                                        },
                                                        cx,
                                                    )),
                                            )
                                        },
                                    )
                                    .when(self.busy && self.listing.is_none(), |body| {
                                        body.child(crate::ui::pending::overlay(
                                            "directory-pending",
                                            cx,
                                        ))
                                    }),
                            )
                            .child(
                                div()
                                    .h(px(28.))
                                    .flex_shrink_0()
                                    .border_t_1()
                                    .border_color(cx.theme().border)
                                    .px_3()
                                    .h_flex()
                                    .items_center()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.status()),
                            ),
                    ),
            )
            .when(self.error.is_some(), |body| {
                body.child(
                    div()
                        .id("directory-error")
                        .debug_selector(|| "directory-error".into()),
                )
            })
    }
}
impl Picker {
    pub(super) fn footer(&self, picker: Entity<Self>, _: &mut Window, cx: &App) -> DialogFooter {
        let path = self
            .selection(cx)
            .map(|selection| match selection {
                Selection::Directory(path) | Selection::Save(path) => path,
                Selection::Files(paths) => paths.join(", "),
            })
            .unwrap_or_default();
        let label = path.clone();
        let tooltip = path.clone();
        let cancel = picker.clone();
        let confirm = picker.clone();
        DialogFooter::new()
            .w_full()
            .gap_3()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .when(matches!(self.mode, Mode::Save { .. }), |body| {
                        body.child(Input::new(&self.filename).aria_label(tr("directory_name")))
                    })
                    .child(
                        h_flex()
                            .min_w_0()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Folder).size_3p5().flex_shrink_0())
                            .child(
                                div()
                                    .id("directory-selection")
                                    .debug_selector(|| "directory-selection".into())
                                    .min_w_0()
                                    .truncate()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(label)
                                    .tooltip(move |window, cx| {
                                        Tooltip::new(tooltip.clone()).build(window, cx)
                                    }),
                            ),
                    ),
            )
            .child(
                Button::new("directory-cancel")
                    .label(tr("settings_cancel"))
                    .debug_selector(|| "directory-cancel".into())
                    .on_click(move |_, window, cx| {
                        cancel.update(cx, |picker, cx| picker.close(window, cx))
                    }),
            )
            .child(
                Button::new("directory-confirm")
                    .primary()
                    .label(tr(self.confirm))
                    .loading((self.pending)(cx))
                    .disabled(
                        self.busy
                            || (self.pending)(cx)
                            || !(self.valid)(cx)
                            || self.error.is_some()
                            || self.selection(cx).is_none(),
                    )
                    .debug_selector(|| "directory-confirm".into())
                    .on_click(move |_, window, cx| {
                        confirm.update(cx, |picker, cx| picker.confirm(window, cx))
                    }),
            )
    }
    fn status(&self) -> SharedString {
        let directories = self
            .visible
            .iter()
            .filter(|index| self.entries()[**index].kind == EntryKind::Directory)
            .count();
        let files = self
            .visible
            .iter()
            .filter(|index| self.entries()[**index].kind == EntryKind::File)
            .count();
        let hidden = if self.hidden {
            0
        } else {
            self.entries()
                .iter()
                .filter(|entry| entry.name.starts_with('.'))
                .count()
        };
        if hidden == 0 {
            rust_i18n::t!("directory_counts", directories = directories, files = files)
                .to_string()
                .into()
        } else {
            rust_i18n::t!(
                "directory_counts_hidden",
                directories = directories,
                files = files,
                hidden = hidden
            )
            .to_string()
            .into()
        }
    }
}
