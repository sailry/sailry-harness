//! One feature catalog for the rail and the complete navigation menu.
use super::*;
use crate::plugins::navigation::Entry;

#[derive(Clone)]
pub(in crate::shell) enum Destination {
    Page(Page),
    Plugin(Entry),
}

#[derive(Clone)]
pub(in crate::shell) struct Feature {
    pub key: String,
    pub label: SharedString,
    pub icon: Icon,
    pub destination: Destination,
    pub default_pin: bool,
    pub default_order: u16,
}

impl From<Entry> for Feature {
    fn from(entry: Entry) -> Self {
        Self {
            // Pins belong to this controller, independent of host and package revision.
            key: format!(
                "plugin:{}:{}",
                match entry.scope {
                    sailry_protocol::plugin::Scope::Host => "host",
                    sailry_protocol::plugin::Scope::Desktop => "desktop",
                },
                entry.package.name
            ),
            label: entry.label.clone().into(),
            icon: crate::assets::icons::icon(&entry.icon),
            default_pin: entry.navigation.pinned,
            default_order: entry.navigation.order,
            destination: Destination::Plugin(entry),
        }
    }
}

impl Feature {
    pub fn pinned(&self, cx: &App) -> bool {
        self.fixed()
            || crate::preferences::data(cx)
                .feature_pins
                .and_then(|pins| pins.get(&self.key).copied())
                .unwrap_or(self.default_pin)
    }

    pub fn fixed(&self) -> bool {
        matches!(self.destination, Destination::Page(Page::Conversation))
    }

    pub fn last(&self) -> bool {
        matches!(self.destination, Destination::Page(Page::Plugins))
    }

    pub fn selector(&self) -> String {
        match &self.destination {
            Destination::Page(page) => format!("navigation-{}", page.key()),
            Destination::Plugin(entry) => entry.selector(),
        }
    }

    pub fn selected(&self, shell: &Shell, cx: &App) -> bool {
        match &self.destination {
            Destination::Page(page) => shell.page == *page,
            Destination::Plugin(entry)
                if entry.navigation.surface
                    == sailry_protocol::plugin::desktop::Surface::Settings =>
            {
                shell.page == Page::Settings
                    && shell.settings.read(cx).selected_plugin_settings()
                        == Some(&entry.package.name)
            }
            Destination::Plugin(entry) => {
                shell.page == Page::Plugin
                    && shell
                        .extensions
                        .as_ref()
                        .is_some_and(|state| state.selected.as_ref() == Some(entry))
            }
        }
    }

    pub fn open(&self, shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
        match &self.destination {
            Destination::Page(page) => shell.navigate(*page, window, cx),
            Destination::Plugin(entry) => shell.open_extension(entry.clone(), window, cx),
        }
    }

    pub fn toggle_pin(&self, cx: &mut App) {
        if self.fixed() {
            return;
        }
        let pinned = !self.pinned(cx);
        crate::preferences::update(cx, |data| {
            data.feature_pins
                .get_or_insert_default()
                .insert(self.key.clone(), pinned);
        });
        cx.refresh_windows();
    }
}

impl Shell {
    #[cfg(target_os = "macos")]
    pub(crate) fn application_navigation(&self, cx: &App) -> Vec<crate::app_menu::Navigation> {
        self.features(cx)
            .into_iter()
            .map(|feature| crate::app_menu::Navigation {
                key: feature.key,
                label: feature.label,
            })
            .collect()
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn open_feature(
        &mut self,
        action: &crate::app_menu::OpenFeature,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(feature) = self
            .features(cx)
            .into_iter()
            .find(|feature| feature.key == action.0)
        {
            feature.open(self, window, cx);
        }
    }

    pub(in crate::shell) fn features(&self, cx: &App) -> Vec<Feature> {
        let mut features: Vec<_> = Page::ALL
            .into_iter()
            .filter(|page| {
                !(self.live.is_some() && matches!(page, Page::Activity | Page::Files | Page::Git))
                    && !matches!(
                        page,
                        Page::Settings | Page::Terminal | Page::Host | Page::Project
                    )
            })
            .map(|page| {
                let icon = if page == Page::Conversation {
                    Icon::default().path("reicon:messages/chat-round")
                } else if page == Page::Activity {
                    Icon::default().path("reicon:ui/grid")
                } else if page == Page::Plugins {
                    Icon::default().path("reicon:ui/puzzle-piece")
                } else {
                    Icon::new(page.icon())
                };
                Feature {
                    key: format!("page:{}", page.key()),
                    label: page.label(),
                    icon,
                    destination: Destination::Page(page),
                    default_pin: true,
                    default_order: match page {
                        Page::Conversation => 0,
                        Page::Activity => 100,
                        Page::Files => 400,
                        Page::Git => 500,
                        Page::Terminal => 600,
                        _ => 10_000,
                    },
                }
            })
            .collect();
        features.extend(self.extension_entries(cx).into_iter().map(Feature::from));
        order::sort(&mut features, cx);
        features
    }
}
