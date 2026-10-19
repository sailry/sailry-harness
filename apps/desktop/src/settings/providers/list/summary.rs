//! Row facts derived from existing channel data; nothing here is persisted.
use super::super::data::{Channel, Model, Preset};
use sailry_protocol::{
    Authentication,
    conversation::{presets::Source, quota},
};

/// Models shown before the expansion collapses the rest into a count.
pub(super) const CHIPS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuthKind {
    OAuth,
    ApiKey,
    Plugin,
}

impl AuthKind {
    pub fn of(channel: &Channel, authentication: Authentication) -> Self {
        let packaged = channel
            .directory
            .as_ref()
            .is_some_and(|entry| matches!(entry.source, Source::Package { .. }));
        if packaged || channel.preset == Preset::Plugin {
            Self::Plugin
        } else if authentication == Authentication::ApiKey {
            Self::ApiKey
        } else {
            Self::OAuth
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::OAuth => "provider_auth_oauth",
            Self::ApiKey => "provider_auth_key",
            Self::Plugin => "provider_auth_plugin",
        }
    }
}

/// Problems the list can prove from saved configuration alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Status {
    MissingKey,
    SignedOut,
}

impl Status {
    pub fn of(channel: &Channel, authentication: Authentication) -> Option<Self> {
        if channel.credential_configured {
            return None;
        }
        if authentication != Authentication::ApiKey {
            return Some(Self::SignedOut);
        }
        // Keyless custom endpoints are valid, matching the editor's save rule.
        let keyless = channel.preset.custom_endpoint() && !channel.preset.cloud();
        (!keyless).then_some(Self::MissingKey)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::MissingKey => "provider_status_missing_key",
            Self::SignedOut => "provider_status_signed_out",
        }
    }
}

/// What the identity tile shows on its gradient.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    /// A package image that cannot be recolored.
    Image,
    /// A monochrome logo asset painted white.
    Logo(String),
    Letter(char),
}

impl Mark {
    pub fn of(channel: &Channel) -> Self {
        let letter = Self::Letter(monogram(&channel.name));
        if let Some(entry) = &channel.directory
            && entry.source != Source::Builtin
        {
            let image = entry.icon.is_some()
                || entry
                    .preset
                    .logo
                    .as_ref()
                    .is_some_and(|logo| logo.starts_with("https://"));
            return if image { Self::Image } else { letter };
        }
        // Compatible endpoints only borrow a protocol logo, which would misname the service.
        if matches!(
            channel.preset,
            Preset::CompatibleResponses
                | Preset::CompatibleChat
                | Preset::CompatibleAnthropic
                | Preset::CompatibleGemini
                | Preset::Plugin
        ) {
            return letter;
        }
        channel
            .directory
            .as_ref()
            .and_then(|entry| entry.preset.logo.clone())
            .or_else(|| channel.preset.category().logo())
            .map_or(letter, Self::Logo)
    }
}

fn monogram(name: &str) -> char {
    name.chars()
        .find(|character| character.is_alphanumeric())
        .and_then(|character| character.to_uppercase().next())
        .unwrap_or('?')
}

/// The default model leads, then the saved order up to `limit`; returns the hidden count.
pub(super) fn chips<'a>(models: &'a [Model], default: &str, limit: usize) -> (Vec<&'a str>, usize) {
    let mut ids = models
        .iter()
        .filter(|model| model.id == default)
        .chain(models.iter().filter(|model| model.id != default))
        .map(|model| model.id.as_str())
        .collect::<Vec<_>>();
    let hidden = ids.len().saturating_sub(limit);
    ids.truncate(limit);
    (ids, hidden)
}

/// Case-insensitive match on the channel name or any model id.
pub(in crate::settings::providers) fn matches(channel: &Channel, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || channel.name.to_lowercase().contains(&query)
        || channel
            .models
            .iter()
            .any(|model| model.id.to_lowercase().contains(&query))
}

/// Used share in percent, or None once the observed window has reset.
pub(super) fn usage(window: &quota::Window, now: u64) -> Option<f32> {
    window
        .available_at(now)
        .then(|| (window.used_basis_points as f32 / 100.).min(100.))
}
