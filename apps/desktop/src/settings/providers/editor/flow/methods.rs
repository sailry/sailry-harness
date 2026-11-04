//! A brand's API methods keep the directory's declarations and order; account
//! logins open directly from the picker.
use super::*;
use crate::ui::modal_list::{List, Row};

pub(super) fn render(editor: &Editor, cx: &Context<Editor>) -> AnyElement {
    let methods = editor
        .flow
        .brand
        .as_deref()
        .map(|brand| brands::api_methods(&editor.directory, brand))
        .unwrap_or_default();
    let Some(first) = methods.first() else {
        return div().into_any_element();
    };
    let context = h_flex()
        .w_full()
        .min_w_0()
        .items_center()
        .gap_3()
        .pb(px(22.))
        .child(presets::render_icon(first, rems(1.8125)))
        .child(
            div()
                .truncate()
                .text_lg()
                .font_medium()
                .child(brand_label(first)),
        );
    let blocked = editor.navigation_blocked();
    let list = List::new("provider-methods").rows(methods.into_iter().map(|entry| {
        let route = &entry.preset.routes[0];
        let access = Access::of(
            presets::kind(&entry, route),
            false,
            presets::custom_endpoint(&entry, route),
        );
        Row::new(
            format!("provider-method-{}", entry.preset.id),
            presets::label(&entry.preset),
        )
        .subtitle(tr(access.key()))
        .chevron()
        .disabled(blocked)
        .on_click(cx.listener(move |editor, _, window, cx| {
            editor.choose_entry(entry.clone(), window, cx);
        }))
    }));
    v_flex()
        .w_full()
        .px_7()
        .pt(px(15.))
        .pb(px(30.))
        .child(context)
        .child(list)
        .into_any_element()
}
