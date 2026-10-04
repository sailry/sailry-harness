//! Script access to the shared card composition, without package-specific fields.
use crate::ui::card_list::{self, Column};
use gpui_kit::IntoElement as _;
use gpui_shell::{HostModule, HostValue};

#[cfg(test)]
mod tests;

pub(super) fn extend(module: HostModule) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        r#"
            /** Cards share spacing and use the enclosing page's scroll container. */
            export const CardList: { new(id: string): import("gpui-kit").Element };
            /** Non-selectable Kit card; children are CardColumn elements with their own controls. */
            export const CardRow: { new(id: string, props?: { row_id?: string }): import("gpui-kit").Element };
            /** Shared column widths and alignment; content and callbacks remain with the caller. */
            export const CardColumn: { new(id: string, props?: { variant?: "leading" | "title" | "metadata" | "wide_metadata" | "control" | "content" | "inline" | "actions"; spacing?: "compact" | "regular" }): import("gpui-kit").Element };
            /** Flexible two-line title/subtitle; children are optional leading controls. */
            export const CardSummary: { new(id: string, props: { title: string; subtitle: string; icon?: string }): import("gpui-kit").Element };
        "#
    );
    module
        .component("CardList", |mut args, _, _| {
            card_list::list(args.id().to_owned(), args.take_children()).into_any_element()
        })
        .component("CardRow", |mut args, _, cx| {
            let id = args.id().to_owned();
            let row_id = args
                .props()
                .get("row_id")
                .and_then(HostValue::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{id}-row"));
            card_list::row(id, row_id, args.take_children(), cx).into_any_element()
        })
        .component("CardSummary", |mut args, _, cx| {
            let props = args.props();
            let title = props
                .get("title")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let subtitle = props
                .get("subtitle")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let icon = props
                .get("icon")
                .and_then(HostValue::as_str)
                .map(str::to_owned);
            card_list::summary(
                args.id().to_owned(),
                title,
                subtitle,
                icon,
                args.take_children(),
                cx,
            )
            .into_any_element()
        })
        .component("CardColumn", |mut args, _, cx| {
            let variant = match args.props().get("variant").and_then(HostValue::as_str) {
                Some("leading") => Column::Leading,
                Some("title") => Column::Title,
                Some("metadata") => Column::Metadata,
                Some("wide_metadata") => Column::WideMetadata,
                Some("control") => Column::Control,
                Some("inline") => Column::Inline,
                Some("actions") => Column::Actions {
                    regular: args.props().get("spacing").and_then(HostValue::as_str)
                        == Some("regular"),
                },
                _ => Column::Content,
            };
            card_list::column(args.id().to_owned(), variant, args.take_children(), cx)
                .into_any_element()
        })
        .declarations(declarations)
}
