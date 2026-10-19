use super::*;
use gpui_kit::component::{
    group_box::{GroupBox, GroupBoxVariants},
    separator::Separator,
};

pub(super) fn row(
    id: String,
    icon: IconName,
    title: impl Into<SharedString>,
    subtitle: Option<String>,
    trailing: impl IntoElement,
    cx: &App,
) -> Stateful<Div> {
    h_flex()
        .id(SharedString::from(id))
        .w_full()
        .min_w_0()
        .min_h(px(48.))
        .px_3()
        .py_2()
        .gap_3()
        .child(
            Icon::new(icon)
                .size_4()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().text_sm().font_normal().truncate().child(title.into()))
                .when_some(subtitle, |view, text| {
                    view.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .font_family(cx.theme().mono_font_family.clone())
                            .truncate()
                            .child(text),
                    )
                }),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(trailing),
        )
}

pub(super) fn section(
    key: &'static str,
    count: Option<usize>,
    rows: Vec<AnyElement>,
    cx: &App,
) -> AnyElement {
    let mut content = v_flex().w_full().min_w_0().gap_0();
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            content = content.child(Separator::horizontal());
        }
        content = content.child(row);
    }
    GroupBox::new()
        .id(key)
        .min_w_0()
        .flex_shrink_0()
        .outline()
        .gap_2()
        .title(
            h_flex()
                .gap_2()
                .text_xs()
                .font_normal()
                .text_color(cx.theme().muted_foreground)
                .child(tr(key))
                .when_some(count, |view, count| view.child(count.to_string())),
        )
        .content_style(StyleRefinement::default().p_0().gap_0())
        .child(content)
        .into_any_element()
}

pub(super) fn tag(label: impl Into<SharedString>) -> AnyElement {
    gpui_kit::component::tag::Tag::secondary()
        .small()
        .child(label.into())
        .into_any_element()
}
