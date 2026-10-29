//! Connection methods keep the directory's declarations and order within each access group.
use super::*;
use sailry_protocol::conversation::presets::Entry;

fn sections(choices: Vec<Entry>) -> [Vec<Entry>; 2] {
    let (accounts, api) = choices.into_iter().partition(|entry| {
        presets::authentication(entry) != sailry_protocol::Authentication::ApiKey
    });
    [accounts, api]
}

pub(super) fn render(editor: &Editor, cx: &Context<Editor>) -> AnyElement {
    let choices = presets::groups(&editor.directory)
        .into_iter()
        .find(|(key, _)| Some(key.as_str()) == editor.flow.brand.as_deref())
        .map(|(_, choices)| choices)
        .unwrap_or_default();
    let Some(first) = choices.first() else {
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
    let sections = sections(choices);
    v_flex()
        .w_full()
        .px_7()
        .pt(px(15.))
        .pb(px(30.))
        .child(context)
        .child(
            v_flex().gap(px(22.)).children(sections.into_iter().enumerate().filter_map(
                |(index, choices)| {
                    if choices.is_empty() {
                        return None;
                    }
                    let account = index == 0;
                    Some(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if account { Access::Login } else { Access::Key }.icon().size_4())
                                    .child(tr(if account { "provider_access_login" } else { "provider_access_api" })),
                            )
                            .child(
                                v_flex()
                                    .w_full()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .rounded(cx.theme().radius_lg)
                                    .overflow_hidden()
                                    .bg(cx.theme().secondary)
                                    .children(choices.into_iter().enumerate().map(|(position, entry)| {
                                        let route = &entry.preset.routes[0];
                                        let access = Access::of(
                                            presets::kind(&entry, route),
                                            account,
                                            presets::custom_endpoint(&entry, route),
                                        );
                                        let label = presets::label(&entry.preset);
                                        div()
                                            .w_full()
                                            .when(position > 0, |row| row.border_t_1().border_color(cx.theme().border))
                                            .child(
                                                Button::new(SharedString::from(format!("provider-method-{}", entry.preset.id)))
                                                    .ghost()
                                                    .rounded(gpui_kit::component::button::ButtonRounded::None)
                                                    .w_full()
                                                    .h(px(if account { 53. } else { 70. }))
                                                    .px_4()
                                                    .disabled(editor.navigation_blocked())
                                                    .accessibility_label(label.clone())
                                                    .child(
                                                        h_flex()
                                                            .size_full()
                                                            .min_w_0()
                                                            .items_center()
                                                            .gap_3()
                                                            .child(
                                                                v_flex()
                                                                    .flex_1()
                                                                    .min_w_0()
                                                                    .items_start()
                                                                    .gap_0p5()
                                                                    .child(div().max_w_full().truncate().text_sm().font_medium().child(label))
                                                                    .when(!account, |column| column.child(
                                                                        div().max_w_full().truncate().text_xs().text_color(cx.theme().muted_foreground).child(tr(access.key()))
                                                                    )),
                                                            )
                                                            .child(Icon::new(IconName::ChevronRight).size_4().text_color(cx.theme().muted_foreground)),
                                                    )
                                                    .on_click(cx.listener(move |editor, _, window, cx| {
                                                        editor.choose_entry(entry.clone(), window, cx);
                                                    })),
                                            )
                                    })),
                            ),
                    )
                },
            )),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preserves_account_and_api_declarations() {
        let choices = presets::groups(&presets::bundled())
            .into_iter()
            .find(|(key, _)| key == "openai")
            .unwrap()
            .1;
        let [accounts, api] = sections(choices.clone());
        assert_eq!(
            accounts
                .iter()
                .map(|entry| entry.preset.id.as_str())
                .collect::<Vec<_>>(),
            ["chat-gpt-oauth", "chat-gpt"]
        );
        assert_eq!(
            api.iter()
                .map(|entry| entry.preset.id.as_str())
                .collect::<Vec<_>>(),
            ["openai", "compatible-responses"]
        );
        assert!(accounts.iter().all(|entry| entry.login.is_none()));
        for entry in accounts.iter().chain(&api) {
            assert!(choices.contains(entry));
        }
        assert_eq!(accounts.len() + api.len(), choices.len());
    }
}
