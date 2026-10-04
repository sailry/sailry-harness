// Adapted from Bezel 4a7505ab, crates/markdown (MIT). See third_party_licenses/bezel.md.
//! Bookmark presentation metadata. The renderer uses the URL and host fallback
//! without fetching link previews or installing a global provider.

use gpui::SharedString;
use gpui_kit as gpui;

/// What a bookmark paints beyond the URL it already has.
///
/// Absent metadata leaves the card's URL and host visible.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    pub title: Option<SharedString>,
    pub description: Option<SharedString>,
    pub image: Option<SharedString>,
    pub icon: Option<SharedString>,
    /// The footer's identity where the host is not the most specific one — a
    /// repository, a subreddit. Which path names a *unit* is the app's
    /// knowledge, not this crate's.
    pub label: Option<SharedString>,
}

/// The host without its `www.` prefix.
pub(crate) fn host(url: &str) -> &str {
    let after = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = after.split(['/', '?', '#']).next().unwrap_or(after);
    host.strip_prefix("www.").unwrap_or(host)
}
