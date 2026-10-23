//! Row facts derived from existing channel data; nothing here is persisted.
use super::super::data::{Channel, Model, Preset};
use sailry_protocol::{
    Authentication,
    conversation::{presets::Source, quota},
    plugin::ui::Detail,
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

/// Remaining share, or None once the observed window has reset.
pub(super) fn remaining(window: &quota::Window, now: u64) -> Option<f32> {
    window
        .available_at(now)
        .then(|| window.remaining_basis_points() as f32 / 100.)
}

/// Scoped public account identity takes precedence over the existing channel type.
pub(super) fn subtitle(channel: &Channel) -> gpui_kit::SharedString {
    if let Some(label) = channel
        .account_label
        .as_ref()
        .filter(|label| !label.trim().is_empty())
    {
        return label.clone().into();
    }
    channel.label()
}

/// One usage column line; the remaining details appear on hover.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Line {
    pub value: gpui_kit::SharedString,
    /// Meters only; omitted when blank.
    pub label: Option<gpui_kit::SharedString>,
    /// A meter precedes the value when present.
    pub progress: Option<f32>,
}

/// Meter lines shown before the rest move to the popover.
pub(super) const METERS: usize = 2;

/// Up to two meter lines; without meters, the first value or the metric's own value.
pub(super) fn lines(
    details: &[Detail],
    fallback: Option<gpui_kit::SharedString>,
    locale: &str,
) -> Vec<Line> {
    let meters: Vec<_> = details
        .iter()
        .filter_map(|detail| {
            let progress = crate::conversation::usage::external::progress(detail)?;
            let text = detail.label.label(locale).trim();
            Some(Line {
                value: detail.value.clone().into(),
                label: (!text.is_empty()).then(|| text.to_owned().into()),
                progress: Some(progress),
            })
        })
        .take(METERS)
        .collect();
    if !meters.is_empty() {
        return meters;
    }
    details
        .first()
        .map(|detail| detail.value.clone().into())
        .or(fallback)
        .map(|value| Line {
            value,
            label: None,
            progress: None,
        })
        .into_iter()
        .collect()
}
