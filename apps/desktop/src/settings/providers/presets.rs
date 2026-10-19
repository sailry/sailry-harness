//! Presentation of the shared directory; execution always resolves it on the Node.
use super::data::Preset as Kind;
use gpui_kit::{
    AnyElement, Image, ImageFormat, IntoElement, Rems, Styled,
    component::{Icon, IconName},
    img,
};
use sailry_protocol::{Authentication, conversation::presets::*};
use std::sync::{Arc, OnceLock};

pub(super) fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../../assets/catalogs/model-presets.json"
        ))
        .expect("bundled model presets must be valid")
    })
}

pub(super) fn bundled() -> Vec<Entry> {
    catalog()
        .builtin
        .iter()
        .cloned()
        .map(|preset| Entry {
            source: Source::Builtin,
            preset,
            provenance: Provenance::Bundled,
            package: None,
            icon: None,
            login: None,
            origin: None,
            requested: Default::default(),
        })
        .collect()
}

pub(super) fn label(preset: &Preset) -> String {
    preset
        .locales
        .get::<str>(&rust_i18n::locale())
        .or_else(|| preset.locales.get("en"))
        .unwrap_or(&preset.name)
        .clone()
}

pub(super) fn icon(entry: &Entry) -> Icon {
    match (&entry.source, &entry.preset.logo) {
        (Source::Builtin, Some(path)) => Icon::empty().path(path.clone()),
        _ => Icon::new(IconName::Settings2),
    }
}

pub(super) fn render_icon(entry: &Entry, size: Rems) -> AnyElement {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    if let Some(data) = entry.icon.as_deref()
        && let Some((prefix, data)) = data.split_once(',')
    {
        let format = match prefix {
            "data:image/png;base64" => Some(ImageFormat::Png),
            "data:image/svg+xml;base64" => Some(ImageFormat::Svg),
            _ => None,
        };
        if let Some((format, bytes)) = format.zip(STANDARD.decode(data).ok()) {
            return img(Arc::new(Image::from_bytes(format, bytes)))
                .size(size)
                .into_any_element();
        }
    }
    if let Some(url) = entry
        .preset
        .logo
        .as_ref()
        .filter(|path| path.starts_with("https://"))
    {
        return img(url.clone()).size(size).into_any_element();
    }
    icon(entry).size(size).into_any_element()
}

pub(super) fn brand_icon(entry: &Entry, size: Rems) -> AnyElement {
    if let Some(icon) = entry
        .preset
        .brand_display
        .as_ref()
        .and_then(|display| display.icon.as_ref())
    {
        return crate::assets::icons::icon(icon)
            .size(size)
            .into_any_element();
    }
    render_icon(entry, size)
}

pub(super) fn brand(entry: &Entry) -> String {
    let name = entry.preset.brand.as_deref().unwrap_or(&entry.preset.id);
    match &entry.source {
        Source::Builtin => name.into(),
        Source::Partner => format!("partner:{name}"),
        Source::Package { name: package } => format!("package:{package}:{name}"),
    }
}

pub(super) fn brand_label(entry: &Entry) -> String {
    if let Some(display) = &entry.preset.brand_display {
        return display.label(&rust_i18n::locale()).to_owned();
    }
    if entry.source == Source::Builtin {
        if entry.preset.brand.as_deref() == Some("opencode") {
            return "OpenCode".into();
        }
        if let Some(brand) = &entry.preset.brand
            && let Some(base) = catalog().builtin.iter().find(|value| value.id == *brand)
        {
            return label(base);
        }
    }
    label(&entry.preset)
}

pub(super) fn groups(entries: &[Entry]) -> Vec<(String, Vec<Entry>)> {
    let mut result: Vec<(String, Vec<Entry>)> = Vec::new();
    for entry in entries.iter().filter(|entry| entry.preset.offered) {
        let key = brand(entry);
        if let Some((_, choices)) = result.iter_mut().find(|(value, _)| *value == key) {
            choices.push(entry.clone());
        } else {
            result.push((key, vec![entry.clone()]));
        }
    }
    result
}

pub(super) fn kind(entry: &Entry, route: &Route) -> Kind {
    if entry.source == Source::Builtin {
        if let Some(super::data::Category::Vendor(vendor)) = super::data::Category::ALL
            .into_iter()
            .find(|category| category.logo() == entry.preset.logo && entry.preset.logo.is_some())
        {
            return Kind::Hosted(vendor);
        }
        match entry.preset.id.as_str() {
            "openai" => return Kind::OpenAi,
            "anthropic" => return Kind::Anthropic,
            "gemini" => return Kind::Gemini,
            _ => {}
        }
    }
    if let Auth::Builtin { authentication } = entry.preset.auth {
        return match authentication {
            Authentication::ChatGptOAuth => Kind::ChatGptOAuth,
            Authentication::ChatGpt => Kind::ChatGpt,
            Authentication::ClaudeOAuth => Kind::ClaudeOAuth,
            Authentication::Copilot if route.protocol == Protocol::OpenAiResponses => {
                Kind::CopilotResponses
            }
            Authentication::Copilot => Kind::Copilot,
            _ => api_kind(route.protocol),
        };
    }
    api_kind(route.protocol)
}

fn api_kind(protocol: Protocol) -> Kind {
    match protocol {
        Protocol::Plugin => Kind::Plugin,
        Protocol::OpenAiResponses => Kind::CompatibleResponses,
        Protocol::OpenAiChatCompletions => Kind::CompatibleChat,
        Protocol::Anthropic => Kind::CompatibleAnthropic,
        Protocol::Gemini => Kind::CompatibleGemini,
        Protocol::DeepSeek => Kind::Hosted(super::vendors::Vendor::DeepSeek),
        Protocol::OpenCodeGo => Kind::OpenCodeGo,
        Protocol::OpenCodeZen => Kind::OpenCodeZen,
        Protocol::AzureOpenAi => Kind::AzureOpenAi,
        Protocol::AzureAi => Kind::AzureAi,
    }
}

pub(super) fn reference(entry: &Entry, route: &Route) -> PresetRef {
    PresetRef {
        source: entry.source.clone(),
        id: entry.preset.id.clone(),
        route: Some(route.key()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn preserves_builtin_icons_and_methods() {
        let entries = bundled();
        let groups = groups(&entries);
        let (_, openai) = groups.iter().find(|(key, _)| key == "openai").unwrap();
        assert!(
            openai
                .iter()
                .any(|entry| entry.preset.id == "chat-gpt-oauth")
        );
        assert!(
            openai
                .iter()
                .any(|entry| entry.preset.id == "compatible-responses")
        );
        for entry in entries.iter().filter(|entry| entry.preset.offered) {
            assert!(
                entry
                    .preset
                    .logo
                    .as_ref()
                    .is_some_and(|path| path.starts_with("icons/providers/"))
            );
            assert!(!entry.preset.routes.is_empty());
        }
    }

    #[test]
    fn keeps_package_brands_separate() {
        let mut entries = bundled();
        let mut plugin = entries[0].clone();
        plugin.source = Source::Package {
            name: "sample.models".into(),
        };
        plugin.provenance = Provenance::ThirdParty;
        let key = brand(&plugin);
        entries.push(plugin);
        assert_eq!(
            groups(&entries)
                .iter()
                .filter(|(brand, _)| brand == &key)
                .count(),
            1
        );
        assert_ne!(key, brand(&entries[0]));
    }
}
