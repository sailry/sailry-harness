//! Connection methods keep the directory's declarations and order within each access group.
use super::*;
use crate::ui::modal_list::{List, Row};
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
    let blocked = editor.navigation_blocked();
    let lists = sections(choices)
        .into_iter()
        .enumerate()
        .filter(|(_, choices)| !choices.is_empty())
        .map(|(index, choices)| {
            let account = index == 0;
            List::new(format!("provider-methods-{index}"))
                .icon(if account { Access::Login } else { Access::Key }.icon())
                .heading(tr(if account {
                    "provider_access_login"
                } else {
                    "provider_access_api"
                }))
                .rows(choices.into_iter().map(|entry| {
                    let route = &entry.preset.routes[0];
                    let access = Access::of(
                        presets::kind(&entry, route),
                        account,
                        presets::custom_endpoint(&entry, route),
                    );
                    Row::new(
                        format!("provider-method-{}", entry.preset.id),
                        presets::label(&entry.preset),
                    )
                    .when(!account, |row| row.subtitle(tr(access.key())))
                    .chevron()
                    .disabled(blocked)
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.choose_entry(entry.clone(), window, cx);
                    }))
                }))
        })
        .collect::<Vec<_>>();
    v_flex()
        .w_full()
        .px_7()
        .pt(px(15.))
        .pb(px(30.))
        .child(context)
        .child(v_flex().gap(px(22.)).children(lists))
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
            ["chat-gpt-oauth"]
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
