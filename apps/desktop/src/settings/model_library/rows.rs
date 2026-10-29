//! Kit owns dialog scrolling and pagination; catalog rows retain their independent controls.
use super::*;
use crate::settings::entry::Entry;
use gpui_kit::component::{
    Selectable,
    button::Button,
    list::{ListDelegate, ListState},
    progress::Progress,
    separator::Separator,
};
use gpui_kit::prelude::FluentBuilder as _;

pub(super) const ROW_HEIGHT: f32 = 4.5;

pub(super) struct Rows {
    pub(super) owner: WeakEntity<Library>,
}

// List requires Selectable even when row selection is disabled. This adapter
// preserves Entry's control-only interaction instead of ListItem's row hover.
#[derive(IntoElement)]
pub(super) struct Row {
    entry: Entry,
    divider: bool,
}

impl Selectable for Row {
    fn selected(self, _: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        false
    }
}

impl RenderOnce for Row {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .relative()
            .w_full()
            .h(rems(ROW_HEIGHT))
            .child(self.entry)
            .when(self.divider, |row| {
                row.child(
                    Separator::horizontal()
                        .absolute()
                        .bottom_0()
                        .w_full()
                        .h(px(1.)),
                )
            })
    }
}

impl ListDelegate for Rows {
    type Item = Row;

    fn items_count(&self, _: usize, cx: &App) -> usize {
        self.owner
            .upgrade()
            .map_or(0, |owner| owner.read(cx).items.len())
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }

    fn has_more(&self, cx: &App) -> bool {
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.read(cx).has_more(cx))
    }

    fn load_more_threshold(&self) -> usize {
        3
    }

    fn load_more(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.update(cx, |library, cx| library.search(true, window, cx));
        }
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let owner = self.owner.upgrade()?;
        let library = owner.read(cx);
        let row = index.row;
        Some(Row {
            divider: row + 1 < library.items.len(),
            entry: result(library, &owner, row, cx)?,
        })
    }
}

/// Splits `owner/name` so the row leads with the model and keeps its owner secondary.
fn title(repository: &str) -> (&str, Option<&str>) {
    match repository.split_once('/') {
        Some((owner, name)) if !owner.is_empty() && !name.is_empty() => (name, Some(owner)),
        _ => (repository, None),
    }
}

/// One catalog result shared by the dialog list and the inline catalog.
pub(super) fn result(
    library: &Library,
    owner: &Entity<Library>,
    row: usize,
    cx: &App,
) -> Option<Entry> {
    let item = library.items.get(row)?;
    let status = library.target.source_status(&item.source, cx);
    let installed = matches!(status, ModelStatus::Installed);
    let locked = !library.available(cx);
    let installing = matches!(status, ModelStatus::Downloading(_) | ModelStatus::Checking)
        || (library.installing.as_ref() == Some(&item.source)
            && (library.loading || library.target.busy(cx)));
    let downloading = locked || library.target.busy(cx);
    let (name, publisher) = title(&item.source.repository);
    let details = owner.clone();
    let install = owner.clone();
    let mut entry = Entry::new(
        format!("model-library-result-{row}"),
        v_flex()
            .min_w_0()
            .gap_1()
            .child(div().font_medium().truncate().child(name.to_owned()))
            .when_some(publisher, |content, publisher| {
                content.child(
                    div()
                        .truncate()
                        .text_color(cx.theme().muted_foreground)
                        .child(publisher.to_owned()),
                )
            }),
    )
    .action(
        Button::new(format!("model-library-details-{row}"))
            .icon(IconName::Info)
            .accessibility_label(tr("knowledge_model_details"))
            .tooltip(tr("knowledge_model_details"))
            .disabled(locked)
            .debug_selector(move || format!("model-library-details-{row}"))
            .on_click(move |_, window, cx| super::details::open(details.clone(), row, window, cx)),
    );
    // A registered transfer replaces the download action with its progress and cancellation.
    if let Some(model) = library
        .target
        .source_entry(&item.source, cx)
        .filter(|model| matches!(model.status, ModelStatus::Downloading(_)))
    {
        let ModelStatus::Downloading(progress) = model.status else {
            unreachable!("filtered to downloading models")
        };
        let target = library.target.clone();
        let key = model.key.clone();
        return Some(
            entry
                .control(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Progress::new(SharedString::from(format!(
                                "model-library-progress-{row}"
                            )))
                            .when_some(progress, |bar, progress| bar.value(progress.percent))
                            .loading(progress.is_none())
                            .small()
                            .w_24()
                            .accessibility_label(tr("knowledge_downloading")),
                        )
                        .when_some(progress, |control, progress| {
                            control.child(
                                div()
                                    .w_10()
                                    .text_right()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{:.0}%", progress.percent)),
                            )
                        }),
                )
                .action(
                    Button::new(format!("model-library-cancel-{row}"))
                        .icon(IconName::CircleX)
                        .accessibility_label(tr("settings_cancel"))
                        .tooltip(tr("settings_cancel"))
                        .disabled(model.cancelling)
                        .debug_selector(move || format!("model-library-cancel-{row}"))
                        .on_click(move |_, window, cx| target.cancel(key.clone(), window, cx)),
                ),
        );
    }
    entry = entry.action(
        Button::new(format!("model-library-install-{row}"))
            .icon(
                Icon::new(if installed {
                    gpui_kit::assets::IconName::Check
                } else {
                    gpui_kit::assets::IconName::Download
                })
                .text_color(if installed {
                    cx.theme().muted_foreground
                } else {
                    cx.theme().primary
                }),
            )
            .accessibility_label(tr(if installed {
                "knowledge_installed"
            } else {
                "knowledge_install"
            }))
            .tooltip(tr(if installed {
                "knowledge_installed"
            } else {
                "knowledge_install"
            }))
            .loading(installing)
            .disabled(downloading || installed || installing)
            .debug_selector(move || format!("model-library-install-{row}"))
            .on_click(move |_, window, cx| {
                install.update(cx, |library, cx| library.install(row, window, cx))
            }),
    );
    Some(entry)
}
