//! Shared non-selectable card rows. Kit owns the surface and child controls.
//! Columns own the accepted spacing and widths; callers supply content and actions.
use gpui_kit::{
    component::{
        group_box::{GroupBox, GroupBoxVariants as _},
        *,
    },
    *,
};

#[derive(Clone, Copy, Default)]
pub(crate) enum Column {
    Leading,
    Title,
    Metadata,
    WideMetadata,
    Control,
    #[default]
    Content,
    Inline,
    Actions {
        regular: bool,
    },
}

pub(crate) fn list(id: String, children: Vec<AnyElement>) -> impl IntoElement {
    v_flex()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .w_full()
        .min_w_0()
        .gap_3()
        .children(children)
}

pub(crate) fn row(
    id: String,
    row_id: String,
    children: Vec<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let surface = format!("{id}-box");
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .w_full()
        .child(frame(
            surface,
            h_flex()
                .id(SharedString::from(row_id.clone()))
                .debug_selector(move || row_id.clone())
                .w_full()
                .min_w_0()
                .items_center()
                .py_3()
                .gap_3()
                .children(children),
            cx,
        ))
}

pub(crate) fn column(
    id: String,
    variant: Column,
    children: Vec<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let cell = if matches!(variant, Column::Actions { .. }) {
        h_flex()
    } else {
        div()
    }
    .id(SharedString::from(id.clone()))
    .debug_selector(move || id.clone());
    let cell = match variant {
        Column::Leading => cell.flex_shrink_0(),
        Column::Title => cell.flex_1().min_w_24().truncate().text_sm(),
        Column::Metadata | Column::WideMetadata => {
            let cell = if matches!(variant, Column::WideMetadata) {
                cell.w_48()
            } else {
                cell.w_32()
            };
            cell.flex_shrink_0()
                .min_w_0()
                .truncate()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
        }
        Column::Content => cell.flex_1().min_w_0(),
        Column::Control => cell
            .w_12()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center(),
        Column::Inline => cell.text_sm().text_color(cx.theme().muted_foreground),
        Column::Actions { regular } => {
            let cell = cell.flex_shrink_0();
            if regular { cell.gap_3() } else { cell.gap_2() }
        }
    };
    cell.children(children)
}

/// Flexible two-line content with an optional leading control and subtitle icon.
pub(crate) fn summary(
    id: String,
    title: String,
    subtitle: String,
    icon: Option<String>,
    children: Vec<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let title_id = format!("{id}-title");
    let subtitle_id = format!("{id}-subtitle");
    let icon_id = format!("{id}-icon");
    h_flex()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_3()
        .children(
            children
                .into_iter()
                .map(|child| div().flex_shrink_0().child(child)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .debug_selector(move || title_id.clone())
                        .truncate()
                        .text_sm()
                        .child(title),
                )
                .child(
                    h_flex()
                        .debug_selector(move || subtitle_id.clone())
                        .min_w_0()
                        .gap_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .children(icon.map(|path| {
                            div()
                                .debug_selector(move || icon_id.clone())
                                .size_4()
                                .flex_shrink_0()
                                .child(Icon::empty().path(path).size_4())
                        }))
                        .child(div().min_w_0().truncate().child(subtitle)),
                ),
        )
}

/// The same Kit surface wraps settings groups and independent card rows.
pub(crate) fn frame(id: impl Into<SharedString>, content: impl IntoElement, cx: &App) -> GroupBox {
    GroupBox::new()
        .id(id.into())
        .fill()
        .gap_2()
        .content_style(
            StyleRefinement::default()
                .p_0()
                .gap_0()
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .rounded(cx.theme().radius_lg)
                .overflow_hidden()
                .child(div().px_4().child(content)),
        )
}
