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
    presentation(
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
            .collect(),
    )
}

/// An empty Node snapshot stays empty; only an unbound preview uses bundled entries.
pub(super) fn available(entries: Option<&[Entry]>) -> Vec<Entry> {
    entries.map_or_else(bundled, |entries| presentation(entries.to_vec()))
}

/// Resolve group presentation from the same directory snapshot as its methods.
pub(super) fn presentation(mut entries: Vec<Entry>) -> Vec<Entry> {
    let displays = entries
        .iter()
        .map(|entry| {
            entry.preset.brand_display.clone().or_else(|| {
                let brand = entry.preset.brand.as_ref()?;
                entries
                    .iter()
                    .find(|base| base.source == entry.source && base.preset.id == *brand)
                    .map(|base| display(&base.preset))
            })
        })
        .collect::<Vec<_>>();
    for (entry, display) in entries.iter_mut().zip(displays) {
        entry.preset.brand_display = display;
    }
    entries
}

fn display(preset: &Preset) -> sailry_protocol::plugin::desktop::Navigation {
    preset
        .brand_display
        .clone()
        .unwrap_or_else(|| sailry_protocol::plugin::desktop::Navigation {
            label: preset.name.clone(),
            locales: preset.locales.clone(),
            icon: None,
        })
}

/// Match a saved channel without a directory reference to its declared type.
pub(crate) fn for_kind(kind: Kind) -> Option<Entry> {
    bundled().into_iter().find(|entry| {
        entry
            .preset
            .routes
            .iter()
            .any(|route| self::kind(entry, route) == kind)
    })
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

/// One directory type's glyph, resolved the same way wherever the type appears.
#[derive(Debug, PartialEq)]
enum Glyph<'a> {
    /// The package's declared brand glyph; monochrome, it follows the text color.
    Declared(&'a sailry_protocol::plugin::desktop::Icon),
    Image(ImageFormat, Vec<u8>),
    Remote(&'a str),
    Fallback,
}

fn glyph(entry: &Entry) -> Glyph<'_> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    if let Some(icon) = entry
        .preset
        .brand_display
        .as_ref()
        .and_then(|display| display.icon.as_ref())
    {
        return Glyph::Declared(icon);
    }
    if let Some((prefix, data)) = entry.icon.as_deref().and_then(|data| data.split_once(',')) {
        let format = match prefix {
            "data:image/png;base64" => Some(ImageFormat::Png),
            "data:image/svg+xml;base64" => Some(ImageFormat::Svg),
            _ => None,
        };
        if let Some((format, bytes)) = format.zip(STANDARD.decode(data).ok()) {
            return Glyph::Image(format, bytes);
        }
    }
    entry
        .preset
        .logo
        .as_deref()
        .filter(|path| path.starts_with("https://"))
        .map_or(Glyph::Fallback, Glyph::Remote)
}

pub(crate) fn render_icon(entry: &Entry, size: Rems) -> AnyElement {
    match glyph(entry) {
        Glyph::Declared(icon) => crate::assets::icons::icon(icon)
            .size(size)
            .into_any_element(),
        Glyph::Image(format, bytes) => img(Arc::new(Image::from_bytes(format, bytes)))
            .size(size)
            .into_any_element(),
        Glyph::Remote(url) => img(url.to_owned()).size(size).into_any_element(),
        Glyph::Fallback => icon(entry).size(size).into_any_element(),
    }
}

/// Resolve type presentation from the already loaded package inventory.
/// Authorization and installation provenance still resolve on the Node.
pub(crate) fn installed(
    reference: Option<&PresetRef>,
    packages: &std::collections::BTreeMap<String, sailry_protocol::plugin::Info>,
) -> Option<Entry> {
    let reference = reference?;
    let Source::Package { name } = &reference.source else {
        return None;
    };
    let info = packages.get(name)?;
    if !info.summary.enabled || !info.issues.is_empty() {
        return None;
    }
    let declarations = &info.extension.as_ref()?.model_providers;
    let mut preset = declarations
        .iter()
        .find(|preset| preset.id == reference.id)?
        .clone();
    if preset.brand_display.is_none() {
        preset.brand_display = preset
            .brand
            .as_ref()
            .and_then(|brand| declarations.iter().find(|base| base.id == *brand))
            .map(display);
    }
    Some(Entry {
        source: reference.source.clone(),
        preset,
        provenance: if matches!(info.origin, Some(sailry_protocol::plugin::Origin::Bundled)) {
            Provenance::Bundled
        } else {
            Provenance::ThirdParty
        },
        package: Some(info.summary.reference()),
        origin: None,
        requested: Default::default(),
        icon: info.icon.clone(),
        login: None,
    })
}

pub(crate) fn brand(entry: &Entry) -> String {
    let name = entry.preset.brand.as_deref().unwrap_or(&entry.preset.id);
    match &entry.source {
        Source::Builtin => name.into(),
        Source::Partner => format!("partner:{name}"),
        Source::Package { name: package } => format!("package:{package}:{name}"),
    }
}

pub(crate) fn brand_label(entry: &Entry) -> String {
    if let Some(display) = &entry.preset.brand_display {
        return display.label(&rust_i18n::locale()).to_owned();
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
    if entry.source == Source::Builtin {
        match (entry.preset.id.as_str(), route.protocol) {
            ("openai", Protocol::OpenAiResponses) => return Kind::OpenAi,
            ("anthropic", Protocol::Anthropic) => return Kind::Anthropic,
            ("gemini", Protocol::Gemini) => return Kind::Gemini,
            _ => {}
        }
        if let Some(vendor) = super::vendors::Vendor::FIRST_PARTY
            .into_iter()
            .find(|vendor| {
                vendor.id() == entry.preset.id
                    && Kind::Hosted(*vendor).api() == route.protocol.api()
            })
        {
            return Kind::Hosted(vendor);
        }
    }
    api_kind(route.protocol)
}

pub(super) fn authentication(entry: &Entry) -> Authentication {
    match entry.preset.auth {
        Auth::Builtin { authentication } => authentication,
        Auth::Plugin { .. } => Authentication::Plugin,
        _ => Authentication::ApiKey,
    }
}

pub(super) fn custom_endpoint(entry: &Entry, route: &Route) -> bool {
    entry.source != Source::Builtin || route.endpoint.is_empty()
}

pub(super) fn product_label(
    preset: &Preset,
    product: sailry_protocol::conversation::connection::Product,
) -> gpui_kit::SharedString {
    preset
        .products
        .iter()
        .find(|item| item.kind == product)
        .and_then(|item| item.display.as_ref())
        .map_or_else(
            || {
                crate::tr(match product {
                    sailry_protocol::conversation::connection::Product::Metered => {
                        "provider_metered"
                    }
                    sailry_protocol::conversation::connection::Product::Coding => "provider_coding",
                })
            },
            |display| display.label(&rust_i18n::locale()).to_owned().into(),
        )
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
mod tests;
