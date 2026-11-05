//! Shared channel picker for the welcome guide and provider editor: account
//! logins open one method each, API brands open their API methods.
use super::presets;
use crate::tr;
use gpui_kit::{
    component::{
        button::{Button, ButtonRounded},
        *,
    },
    *,
};
use sailry_protocol::{Authentication, conversation::presets::Entry};
use std::rc::Rc;

/// What a picker chip opens. Identities resolve against the editor's own
/// directory snapshot, which may load after the welcome guide's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// One account-login method of a brand, configured directly.
    Login { brand: String, id: String },
    /// A brand's API methods.
    Api(String),
}

/// Offered groups ordered by their brand label, as every entry point shows them.
fn sorted(entries: &[Entry]) -> Vec<(String, Vec<Entry>)> {
    let mut groups = presets::groups(entries);
    groups.sort_by_cached_key(|(key, choices)| {
        (
            presets::brand_label(&choices[0]).to_lowercase(),
            key.clone(),
        )
    });
    groups
}

/// Account logins ordered by their chip label, then brands with API methods.
fn split(entries: &[Entry]) -> (Vec<(String, Entry)>, Vec<(String, Vec<Entry>)>) {
    let mut logins = Vec::new();
    let mut brands = Vec::new();
    for (key, choices) in sorted(entries) {
        let (accounts, api): (Vec<_>, Vec<_>) = choices.into_iter().partition(presets::login);
        logins.extend(accounts.into_iter().map(|entry| (key.clone(), entry)));
        if !api.is_empty() {
            brands.push((key, api));
        }
    }
    logins.sort_by_cached_key(|(key, entry)| {
        (
            login_label(entry).to_lowercase(),
            key.clone(),
            entry.preset.id.clone(),
        )
    });
    (logins, brands)
}

/// A brand's offered API methods in declared order.
pub(super) fn api_methods(entries: &[Entry], brand: &str) -> Vec<Entry> {
    presets::groups(entries)
        .into_iter()
        .find(|(key, _)| key == brand)
        .map(|(_, choices)| {
            choices
                .into_iter()
                .filter(|entry| !presets::login(entry))
                .collect()
        })
        .unwrap_or_default()
}

/// The brand and methods a choice opens; a single method configures directly.
pub(super) fn resolve(entries: &[Entry], choice: &Choice) -> Option<(String, Vec<Entry>)> {
    match choice {
        Choice::Login { brand, id } => presets::groups(entries)
            .into_iter()
            .find(|(key, _)| key == brand)?
            .1
            .into_iter()
            .find(|entry| entry.preset.id == *id && presets::login(entry))
            .map(|entry| (brand.clone(), vec![entry])),
        Choice::Api(brand) => {
            let methods = api_methods(entries, brand);
            (!methods.is_empty()).then(|| (brand.clone(), methods))
        }
    }
}

/// Built-in account logins name the account product; their method name stays
/// the preset label everywhere else.
fn login_label(entry: &Entry) -> SharedString {
    match presets::authentication(entry) {
        Authentication::ChatGptOAuth => tr("provider_login_chat_gpt"),
        Authentication::ClaudeOAuth => tr("provider_login_claude"),
        _ => presets::label(&entry.preset).into(),
    }
}

pub(super) fn picker(
    entries: Option<&[Entry]>,
    disabled: bool,
    select: impl Fn(Choice, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let (logins, brands) = split(&presets::available(entries));
    Picker {
        logins,
        brands,
        disabled,
        select: Rc::new(select),
    }
    .into_any_element()
}

type Select = Rc<dyn Fn(Choice, &mut Window, &mut App)>;

#[derive(IntoElement)]
struct Picker {
    logins: Vec<(String, Entry)>,
    brands: Vec<(String, Vec<Entry>)>,
    disabled: bool,
    select: Select,
}

impl Picker {
    fn chip(&self, id: String, entry: &Entry, label: SharedString, choice: Choice) -> Button {
        let select = self.select.clone();
        Button::new(SharedString::from(format!("provider-chip-{id}")))
            .outline()
            // Radii clamp to half the height, which gives a pill.
            .rounded(ButtonRounded::Size(px(999.)))
            .pl(px(10.))
            .pr(px(14.))
            .disabled(self.disabled)
            .accessibility_label(label.clone())
            .child(presets::render_icon(entry, rems(1.)))
            .child(label)
            .on_click(move |_, window, cx| select(choice.clone(), window, cx))
    }
}

impl RenderOnce for Picker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let logins = self
            .logins
            .iter()
            .map(|(brand, entry)| {
                self.chip(
                    format!("login-{brand}-{}", entry.preset.id),
                    entry,
                    login_label(entry),
                    Choice::Login {
                        brand: brand.clone(),
                        id: entry.preset.id.clone(),
                    },
                )
            })
            .collect::<Vec<_>>();
        let brands = self
            .brands
            .iter()
            .map(|(brand, choices)| {
                self.chip(
                    format!("api-{brand}"),
                    &choices[0],
                    presets::brand_label(&choices[0]).into(),
                    Choice::Api(brand.clone()),
                )
            })
            .collect::<Vec<_>>();
        let muted = cx.theme().muted_foreground;
        let group = |key: &str, chips: Vec<Button>| {
            (!chips.is_empty()).then(|| {
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(div().text_xs().text_color(muted).child(tr(key)))
                    .child(h_flex().w_full().flex_wrap().gap_2().children(chips))
            })
        };
        v_flex()
            .id("provider-picker")
            .w_full()
            .gap_5()
            .children(group("provider_access_login", logins))
            .children(group("provider_access_key", brands))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.preset.id.as_str())
            .collect()
    }

    #[test]
    fn splits_logins_from_api_brands() {
        let (logins, brands) = split(&presets::bundled());
        assert_eq!(
            logins
                .iter()
                .map(|(brand, entry)| (brand.as_str(), entry.preset.id.as_str()))
                .collect::<Vec<_>>(),
            [("openai", "chat-gpt-oauth"), ("anthropic", "claude-oauth")]
        );
        assert_eq!(
            logins
                .iter()
                .map(|(_, entry)| login_label(entry))
                .collect::<Vec<_>>(),
            [tr("provider_login_chat_gpt"), tr("provider_login_claude")]
        );
        assert!(
            brands
                .iter()
                .flat_map(|(_, choices)| choices)
                .all(|entry| !presets::login(entry))
        );
        let keys = brands
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>();
        assert!(keys.contains(&"openai") && keys.contains(&"anthropic"));
        let labels = brands
            .iter()
            .map(|(_, choices)| presets::brand_label(&choices[0]).to_lowercase())
            .collect::<Vec<_>>();
        assert!(labels.is_sorted());
    }

    #[test]
    fn keeps_order_across_entry_points() {
        let mut entries = presets::bundled();
        let (logins, brands) = split(&entries);
        entries.reverse();
        let (reversed_logins, reversed) = split(&entries);
        assert_eq!(reversed_logins, logins);
        assert_eq!(
            reversed,
            brands
                .into_iter()
                .map(|(key, mut choices)| {
                    choices.reverse();
                    (key, choices)
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(split(&[]), (Vec::new(), Vec::new()));
    }

    mod resolve {
        use super::*;

        #[test]
        fn opens_login_directly() {
            let entries = presets::bundled();
            let (brand, methods) = resolve(
                &entries,
                &Choice::Login {
                    brand: "anthropic".into(),
                    id: "claude-oauth".into(),
                },
            )
            .unwrap();
            assert_eq!(brand, "anthropic");
            assert_eq!(ids(&methods), ["claude-oauth"]);
            for (brand, id) in [("openai", "openai"), ("anthropic", "chat-gpt-oauth")] {
                let choice = Choice::Login {
                    brand: brand.into(),
                    id: id.into(),
                };
                assert!(resolve(&entries, &choice).is_none(), "{brand}/{id}");
            }
        }

        #[test]
        fn lists_only_api_methods() {
            let entries = presets::bundled();
            for (brand, expected) in [
                ("openai", &["openai", "compatible-responses"][..]),
                ("anthropic", &["anthropic", "compatible-anthropic"]),
                ("gemini", &["gemini", "compatible-gemini"]),
                ("opencode", &["opencode-go", "opencode-zen"]),
                ("deepseek", &["deepseek"]),
            ] {
                let (key, methods) = resolve(&entries, &Choice::Api(brand.into())).unwrap();
                assert_eq!(key, brand);
                assert_eq!(ids(&methods), expected, "{brand}");
            }
            assert!(resolve(&entries, &Choice::Api("missing".into())).is_none());
            assert!(api_methods(&entries, "missing").is_empty());
        }

        #[test]
        fn skips_unoffered_siwc() {
            let entries = presets::bundled();
            let choice = Choice::Login {
                brand: "openai".into(),
                id: "chat-gpt".into(),
            };
            assert!(resolve(&entries, &choice).is_none());
        }
    }
}
