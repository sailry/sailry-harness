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

/// Public account metadata replaces only the login badge, never an API key label.
pub(super) fn badge(channel: &Channel, kind: AuthKind) -> gpui_kit::SharedString {
    if kind != AuthKind::ApiKey
        && let Some(label) = channel
            .account_label
            .as_ref()
            .filter(|label| !label.is_empty())
    {
        return label.clone().into();
    }
    crate::tr(kind.key())
}
