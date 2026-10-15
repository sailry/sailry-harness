//! Kit owns scrolling and pagination; catalog rows retain their independent controls.
use super::*;
use crate::settings::entry::Entry;
use gpui_kit::component::{
    Selectable,
    button::Button,
    list::{ListDelegate, ListState},
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
        let item = library.items.get(row)?;
        let status = library.target.source_status(&item.source, cx);
        let installed = matches!(status, ModelStatus::Installed);
        let locked = !library.available(cx);
        let installing = matches!(status, ModelStatus::Downloading(_) | ModelStatus::Checking)
            || (library.installing.as_ref() == Some(&item.source)
                && (library.loading || library.target.busy(cx)));
        let downloading = locked || library.target.busy(cx);
        let divider = row + 1 < library.items.len();
        let details = owner.clone();
        let install = owner.clone();
        Some(Row {
            divider,
            entry: Entry::new(
                format!("model-library-result-{row}"),
                v_flex()
                    .gap_1()
                    .child(div().font_medium().truncate().child(item.name.clone()))
                    .child(
                        div()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(item.source.repository.clone()),
                    ),
            )
            .action(
                Button::new(format!("model-library-details-{row}"))
                    .icon(IconName::Info)
                    .accessibility_label(tr("knowledge_model_details"))
                    .tooltip(tr("knowledge_model_details"))
                    .disabled(locked)
                    .debug_selector(move || format!("model-library-details-{row}"))
                    .on_click(move |_, window, cx| {
                        super::details::open(details.clone(), row, window, cx)
                    }),
            )
            .action(
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
            ),
        })
    }
}
