//! Kit b79f4ce script bindings omit panel dragging and GroupBox content styling.
//! Adapt those gaps using the same native header and Kit settings surface.
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{separator::Separator, *},
    *,
};
use gpui_shell::{HostModule, HostValue};

mod cards;
mod collapse;
mod scroll;

#[cfg(test)]
mod tests;

pub(super) fn extend(module: HostModule) -> HostModule {
    let module = scroll::extend(collapse::extend(cards::extend(module)));
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        r#"
            export const PanelHeader: { new(id: string, props?: { padding?: number; gap?: number; surface?: "content"; size?: "row"; draggable?: boolean; bordered?: boolean }): import("gpui-kit").Element };
            /** List and card share content; fill_height controls either variant independently. */
            export const EmptyState: { new(id: string, props: { icon: string; label: string; variant?: "panel" | "list" | "card"; fill_height?: boolean; vertical_align?: "start" | "center" }): import("gpui-kit").Element };
            /** Children form a fixed, right-aligned footer; row details preserve selectable full text. */
            export const StatusList: { new(id: string, props: { items: {id:string;text:string;state:"pending"|"in_progress"|"completed"|"skipped"|"paused"|"blocked"|"failed";details?:boolean}[]; empty: string }): import("gpui-kit").Element };
            export const Loading: { new(id: string): import("gpui-kit").Element };
            export const InputSurface: { new(id: string): import("gpui-kit").Element };
            /** With header_action, the first child is the trailing header control. */
            export const SettingsGroup: { new(id: string, props: { title?: string; heading?: boolean; header_action?: boolean }): import("gpui-kit").Element };
            /** The shared centered 800px page, with a scrolling body and arbitrary children. */
            export const SettingsPage: { new(id: string, props: { title: string; description?: string }): import("gpui-kit").Element };
        "#
    );
    module
        .component("StatusList", |mut args, _, _| {
            let items = args
                .props()
                .get("items")
                .and_then(|value| super::host::sdk::values::decode(value).ok())
                .and_then(|value| {
                    serde_json::from_value::<Vec<crate::ui::status_list::Row>>(value).ok()
                })
                .filter(|items| {
                    items.len() <= 4096
                        && items
                            .iter()
                            .all(|row| row.id.len() <= 128 && row.text.len() <= 32768)
                })
                .unwrap_or_default();
            let empty = args
                .props()
                .get("empty")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            crate::ui::status_list::Content {
                id: args.id().to_owned(),
                items,
                empty: empty.into(),
                footer: args.take_children(),
            }
            .into_any_element()
        })
        .component("SettingsPage", |mut args, _, _| {
            let title = args
                .props()
                .get("title")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let description = args
                .props()
                .get("description")
                .and_then(HostValue::as_str)
                .map(str::to_owned);
            crate::ui::settings_page::SettingsPage::new(args.id().to_owned(), title)
                .when_some(description, |page, text| page.description(text))
                .children(args.take_children())
                .into_any_element()
        })
        .component("InputSurface", |mut args, _, cx| {
            let id = args.id().to_owned();
            crate::conversation::input_surface(cx)
                .debug_selector(move || id.clone())
                .gap_2()
                .children(args.take_children())
                .into_any_element()
        })
        .component("Loading", |args, _, cx| {
            let id = args.id().to_owned();
            div()
                .id(id.clone())
                .debug_selector(move || id.clone())
                .absolute()
                .inset_0()
                .occlude()
                .bg(cx.theme().background.opacity(0.8))
                .flex()
                .items_center()
                .justify_center()
                .child(spinner::Spinner::new().large())
                .into_any_element()
        })
        .component("EmptyState", |mut args, _, cx| {
            let icon = args
                .props()
                .get("icon")
                .and_then(HostValue::as_str)
                .unwrap_or_default();
            let label = args
                .props()
                .get("label")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let icon = Icon::empty().path(if icon.contains('/') {
                icon.to_owned()
            } else {
                format!("icons/{icon}.svg")
            });
            let id: SharedString = args.id().to_owned().into();
            let variant = args.props().get("variant").and_then(HostValue::as_str);
            let start = args
                .props()
                .get("vertical_align")
                .and_then(HostValue::as_str)
                == Some("start");
            match variant {
                Some("list" | "card") => {
                    let card = variant == Some("card");
                    let fill_height =
                        args.props().get("fill_height").and_then(HostValue::as_bool) == Some(true);
                    let body = crate::empty_state::list_content(icon, label.into(), id.clone(), cx)
                        .when(fill_height && !card, |body| {
                            body.flex_1().h_full().min_h_0()
                        })
                        .when(start, |body| body.justify_start())
                        .children(args.take_children());
                    if card {
                        crate::empty_state::card_aligned(body, id, fill_height, start, cx)
                            .into_any_element()
                    } else {
                        body.into_any_element()
                    }
                }
                _ => crate::empty_state::content(icon, label.into(), id, cx)
                    .when(start, |body| body.justify_start())
                    .gap_4()
                    .children(args.take_children())
                    .into_any_element(),
            }
        })
        .component("PanelHeader", |mut args, _, cx| {
            let padding = args
                .props()
                .get("padding")
                .and_then(HostValue::as_number)
                .filter(|value| value.is_finite() && *value >= 0. && *value <= f32::MAX as f64);
            // Kit b79f4ce discards chained styles on opaque HostModule components,
            // so native header spacing must cross this boundary as a prop.
            let gap = args
                .props()
                .get("gap")
                .and_then(HostValue::as_number)
                .filter(|value| value.is_finite() && *value >= 0. && *value <= f32::MAX as f64);
            let content =
                args.props().get("surface").and_then(HostValue::as_str) == Some("content");
            let row = args.props().get("size").and_then(HostValue::as_str) == Some("row");
            let draggable =
                args.props().get("draggable").and_then(HostValue::as_bool) != Some(false);
            let bordered = args.props().get("bordered").and_then(HostValue::as_bool) == Some(true);
            crate::header::Header::new(args.id().to_owned(), cx)
                .bordered(bordered)
                .draggable(draggable)
                .when(row, |header| {
                    header.h(gpui_kit::component::Size::Medium.table_row_height())
                })
                .when_some(padding, |header, padding| header.px(px(padding as f32)))
                .when_some(gap, |header, gap| header.gap(px(gap as f32)))
                .when(content, |header| header.bg(cx.theme().background))
                .children(args.take_children())
                .into_any_element()
        })
        .component("SettingsGroup", |mut args, _, cx| {
            let title = args
                .props()
                .get("title")
                .and_then(HostValue::as_str)
                .unwrap_or_default()
                .to_owned();
            let show_heading =
                args.props().get("heading").and_then(HostValue::as_bool) != Some(false);
            let header_action = args
                .props()
                .get("header_action")
                .and_then(HostValue::as_bool)
                == Some(true);
            let mut children = args.take_children().into_iter();
            let action = header_action.then(|| children.next()).flatten();
            let heading = h_flex()
                .w_full()
                .min_h_7()
                .gap_3()
                .flex_wrap()
                .child(div().child(title))
                .when_some(action, |heading, action| {
                    heading.child(h_flex().ml_auto().gap_2().justify_end().child(action))
                });
            let mut rows = v_flex().w_full().min_w_0().gap_0();
            for (index, row) in children.enumerate() {
                if index > 0 {
                    rows = rows.child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(1.))
                            .flex_shrink_0()
                            .child(Separator::horizontal().size_full()),
                    );
                }
                rows = rows.child(row);
            }
            crate::ui::card_list::frame(args.id().to_owned(), rows, cx)
                .when(show_heading, |group| group.title(heading))
                .title_style(
                    StyleRefinement::default()
                        .text_sm()
                        .text_color(cx.theme().foreground),
                )
                .into_any_element()
        })
        .declarations(declarations)
}
