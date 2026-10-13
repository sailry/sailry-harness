use super::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::table::{Column, TableDelegate};

pub(in crate::file_picker) struct Rows {
    pub owner: Option<WeakEntity<Picker>>,
    pub entries: Vec<(usize, FileEntry)>,
    pub selected: BTreeSet<usize>,
    pub width: f32,
    pub filtered: bool,
    pub multiple: bool,
}
impl Rows {
    pub fn new() -> Self {
        Self {
            owner: None,
            entries: Vec::new(),
            selected: BTreeSet::new(),
            width: 400.,
            filtered: false,
            multiple: false,
        }
    }
    fn background(
        &self,
        index: Option<usize>,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let owner = self.owner.clone();
        div()
            .id(("directory-table-row", index.unwrap_or(usize::MAX)))
            .bg(cx.theme().background)
            .when(
                index.is_some_and(|index| self.selected.contains(&index)),
                |row| row.bg(cx.theme().list_active),
            )
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                cx.stop_propagation();
                if let Some(owner) = &owner {
                    let _ = owner.update(cx, |picker, cx| picker.menu(index, event, window, cx));
                }
            })
    }
}
impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        3
    }
    fn rows_count(&self, _: &App) -> usize {
        self.entries.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        let (key, width) = match index {
            0 => ("files_entry_name", self.width),
            1 => ("directory_kind", 120.),
            _ => ("directory_size", 72.),
        };
        Column::new(key, tr(key))
            .width(px(width))
            .min_width(px(width))
            .max_width(px(width))
            .resizable(false)
            .movable(false)
    }
    fn render_header(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        self.background(None, cx)
    }
    fn render_th(
        &mut self,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(tr(match column {
                0 => "files_entry_name",
                1 => "directory_kind",
                _ => "directory_size",
            }))
    }
    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let Some((index, entry)) = self.entries.get(row) else {
            return self.background(None, cx);
        };
        let index = *index;
        let hidden = entry.name.starts_with('.');
        let owner = self.owner.clone();
        self.background(Some(index), cx)
            .debug_selector(move || format!("directory-row-{index}"))
            .when(hidden, |row| row.opacity(0.55))
            .on_click(move |event, window, cx| {
                if let Some(owner) = &owner {
                    let _ = owner.update(cx, |picker, cx| {
                        let modifiers = window.modifiers();
                        picker.select(index, modifiers, window, cx);
                        if event.click_count() == 2 {
                            picker.activate(index, window, cx);
                        }
                    });
                }
            })
    }
    fn render_td(
        &mut self,
        row: usize,
        column: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let (index, entry) = &self.entries[row];
        let index = *index;
        let folder = entry.kind == EntryKind::Directory;
        let owner = self.owner.clone();
        h_flex()
            .size_full()
            .min_w_0()
            .text_xs()
            .text_color(if folder {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(column == 0, |cell| {
                cell.gap_2()
                    .when(self.multiple && entry.kind == EntryKind::File, |cell| {
                        cell.child(
                            Checkbox::new(("directory-selected", index))
                                .accessibility_label(entry.name.clone())
                                .checked(self.selected.contains(&index))
                                .small()
                                .on_change(move |checked, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(owner) = &owner {
                                        let _ = owner.update(cx, |picker, cx| {
                                            if picker.selected.contains(&index) != *checked {
                                                picker.choose(index, true, window, cx);
                                            }
                                        });
                                    }
                                }),
                        )
                    })
                    .child(icon(entry, rems(1.)))
                    .child(div().min_w_0().truncate().child(entry.name.clone()))
            })
            .when(column == 1, |cell| {
                cell.text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(kind(entry))
            })
            .when(column == 2, |cell| {
                cell.text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .justify_end()
                    .child(bytes(entry))
            })
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        self.background(None, cx)
            .size_full()
            .debug_selector(|| "directory-empty".into())
            .child(crate::empty_state::panel(
                IconName::FolderOpen,
                if self.filtered {
                    "directory_no_matches"
                } else {
                    "files_directory_empty"
                },
                cx,
            ))
    }
}
pub(super) fn icon(entry: &FileEntry, size: Rems) -> AnyElement {
    let path = if entry.kind == EntryKind::Directory {
        format!("{}/", entry.name)
    } else {
        entry.name.clone()
    };
    crate::ui::file_icon::render(&path, "", size)
}
fn kind(entry: &FileEntry) -> SharedString {
    if entry.kind == EntryKind::Directory {
        return tr("directory_folder_kind");
    }
    let extension = entry
        .name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "md" | "markdown" => tr("directory_markdown_kind"),
        "toml" => tr("directory_toml_kind"),
        "json" => tr("directory_json_kind"),
        "yaml" | "yml" => tr("directory_yaml_kind"),
        "txt" => tr("directory_text_kind"),
        "png" | "jpg" | "jpeg" | "gif" | "webp" => tr("directory_image_kind"),
        "" => tr("directory_file_kind"),
        _ => rust_i18n::t!(
            "directory_extension_kind",
            extension = extension.to_uppercase()
        )
        .to_string()
        .into(),
    }
}
fn bytes(entry: &FileEntry) -> SharedString {
    if entry.kind == EntryKind::Directory {
        return "—".into();
    }
    let (amount, unit) = if entry.size >= 1_000_000_000 {
        (entry.size as f64 / 1_000_000_000., "GB")
    } else if entry.size >= 1_000_000 {
        (entry.size as f64 / 1_000_000., "MB")
    } else if entry.size >= 1_000 {
        (entry.size as f64 / 1_000., "KB")
    } else {
        (entry.size as f64, "B")
    };
    if unit == "B" {
        format!("{} {unit}", entry.size).into()
    } else {
        format!("{amount:.1} {unit}").into()
    }
}
