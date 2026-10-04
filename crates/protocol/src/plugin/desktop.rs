//! Bounded, portable view resources for a Node-owned desktop extension.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod icon;
pub use icon::Icon;

pub const MAX_FILES: usize = 128;
pub const MAX_BYTES: usize = 256 * 1024;
/// Base64 PNG data; together with escaped text this stays below the Link frame limit.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Manifest {
    /// A bundled JavaScript module; native registrations have no script entry.
    #[schemars(transform = super::schema::resource_path)]
    pub entry: Option<String>,
    /// Optional module implementing declarative controller contributions.
    #[schemars(transform = super::schema::resource_path)]
    pub ui_entry: Option<String>,
    /// Render the captured contribution controller as an overlay layer.
    #[serde(default)]
    pub ui_overlay: bool,
    /// Also contribute controls to assistants owned by other packages.
    #[serde(default)]
    pub ui_shared: bool,
    /// Includes the entry, imported modules and localization resources.
    #[schemars(transform = super::schema::resource_paths)]
    #[serde(default)]
    pub resources: Vec<String>,
    /// Optional main-workspace entry contributed to the application sidebar.
    pub navigation: Option<Navigation>,
    /// Initial rail placement; explicit controller preferences take precedence.
    #[serde(default)]
    pub navigation_options: NavigationOptions,
    /// Optional entry contributed to the conversation resource launcher.
    pub panel: Option<Navigation>,
    /// Named assistants reuse the host's ordinary session execution and controls.
    #[serde(default)]
    pub conversations: Vec<super::conversation::Declaration>,
    /// Resource pages and their declared creation action.
    #[serde(default)]
    pub renderers: Vec<Renderer>,
    /// MIME types whose captured files can be mounted by the package entry.
    #[serde(default)]
    pub previews: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Terminal,
    Browser,
    Documents,
    Git,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Renderer {
    pub resource: ResourceKind,
    pub create: Option<String>,
    /// Optional desktop shortcut for opening this declared resource.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
}

impl Renderer {
    pub fn valid(&self, extension: &super::Extension) -> bool {
        let (creation, actions): (bool, &[super::Action]) = match self.resource {
            ResourceKind::Terminal => (
                self.create.as_ref().is_some_and(|create| {
                    extension.ui.iter().any(|entry| {
                        entry.id == *create
                            && entry.slot == super::ui::Slot::Project
                            && entry.kind == super::ui::Kind::Button
                    })
                }),
                &[
                    super::Action::ReadTerminals,
                    super::Action::ControlTerminals,
                ],
            ),
            ResourceKind::Browser => (
                self.create.is_none(),
                &[super::Action::ReadBrowser, super::Action::ControlBrowser],
            ),
            ResourceKind::Git => (
                self.create.is_none() && extension.scope == super::Scope::Host,
                &[super::Action::ReadGit],
            ),
            ResourceKind::Documents => (
                self.create.is_none() && extension.scope == super::Scope::Host,
                &[super::Action::ReadFiles],
            ),
        };
        creation
            && actions
                .iter()
                .all(|action| extension.actions.contains(action))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
#[schemars(deny_unknown_fields)]
pub struct NavigationOptions {
    pub pinned: bool,
    pub order: u16,
    pub target: NavigationTarget,
    pub surface: Surface,
    pub details: bool,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum NavigationTarget {
    #[default]
    Node,
    Worktree,
}

impl Default for NavigationOptions {
    fn default() -> Self {
        Self {
            pinned: false,
            order: 10_000,
            target: NavigationTarget::Node,
            surface: Surface::Workspace,
            details: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Navigation {
    pub label: String,
    /// Required for workspace navigation; optional for localized text and settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    #[serde(default)]
    pub locales: BTreeMap<String, String>,
}

impl Navigation {
    pub fn label(&self, locale: &str) -> &str {
        self.locales
            .get(locale)
            .map(String::as_str)
            .unwrap_or(&self.label)
    }

    pub fn valid(&self) -> bool {
        let label = |value: &str| {
            !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
        };
        label(&self.label)
            && self.icon.as_ref().is_none_or(Icon::valid)
            && self.locales.len() <= 32
            && self
                .locales
                .iter()
                .all(|(locale, value)| locale.len() <= 32 && label(locale) && label(value))
    }
}

/// Shared contribution for native and packaged settings pages.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct Settings {
    #[serde(default)]
    pub full_width: bool,
    #[serde(default)]
    pub enabled_control: bool,
    #[serde(default)]
    pub placement: SettingsPlacement,
    pub navigation: Navigation,
    /// Absent for native renderers or a schema-generated configuration page.
    #[schemars(transform = super::schema::resource_path)]
    pub entry: Option<String>,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SettingsGroup {
    App,
    Ai,
    #[default]
    Tools,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
#[schemars(deny_unknown_fields)]
pub struct SettingsPlacement {
    pub group: SettingsGroup,
    pub order: u16,
}

impl Default for SettingsPlacement {
    fn default() -> Self {
        Self {
            group: SettingsGroup::Tools,
            order: 10_000,
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    #[default]
    Workspace,
    Settings,
    Composer,
    Project,
}

impl Manifest {
    pub fn valid(&self) -> bool {
        self.entry.as_ref().is_none_or(|entry| {
            (entry.ends_with(".js") || entry.ends_with(".mjs")) && self.resources.contains(entry)
        }) && matches!(
            self.navigation_options.surface,
            Surface::Workspace | Surface::Settings
        ) && (self.navigation_options.surface != Surface::Settings
            || self.navigation.is_some()
                && self.navigation_options.target == NavigationTarget::Node
                && !self.navigation_options.details)
            && super::conversation::valid(&self.conversations)
            && ((!self.navigation_options.details
                && self.navigation_options.target == NavigationTarget::Node)
                || self.entry.is_some() && self.navigation.is_some())
            && self.previews.len() <= 32
            && self.previews.iter().enumerate().all(|(index, mime)| {
                self.entry.is_some()
                    && mime.len() <= 128
                    && mime
                        .split_once('/')
                        .is_some_and(|(kind, subtype)| !kind.is_empty() && !subtype.is_empty())
                    && mime.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || b"/.-+".contains(&byte)
                    })
                    && !self.previews[..index].contains(mime)
            })
            && self.renderers.iter().enumerate().all(|(index, renderer)| {
                self.entry.is_some()
                    && renderer.shortcut.as_ref().is_none_or(|key| {
                        !key.trim().is_empty()
                            && key.len() <= 128
                            && key.split_whitespace().count() <= 2
                            && key
                                .bytes()
                                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
                    })
                    && (match renderer.resource {
                        ResourceKind::Terminal => renderer
                            .create
                            .as_deref()
                            .is_some_and(super::ui::identifier),
                        ResourceKind::Browser | ResourceKind::Documents | ResourceKind::Git => {
                            renderer.create.is_none()
                        }
                    })
                    && !self.renderers[..index]
                        .iter()
                        .any(|other| other.resource == renderer.resource)
            })
            && self
                .navigation
                .as_ref()
                .is_none_or(|entry| entry.valid() && entry.icon.is_some())
            && self.panel.as_ref().is_none_or(Navigation::valid)
            && (!self.ui_overlay || self.ui_entry.is_some())
            && (!self.ui_shared || self.ui_entry.is_some())
            && self.ui_entry.as_ref().is_none_or(|entry| {
                (entry.ends_with(".js") || entry.ends_with(".mjs"))
                    && self.resources.contains(entry)
            })
            && if self.entry.is_none() {
                self.resources.is_empty()
                    || self.navigation.is_none()
                        && self.panel.is_none()
                        && self.conversations.is_empty()
                        && valid_paths(self.resources.iter().map(String::as_str))
            } else {
                valid_paths(self.resources.iter().map(String::as_str))
            }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub package: super::Reference,
    pub entry: String,
    /// Complete UTF-8 files, never truncated or local filesystem locations.
    pub files: BTreeMap<String, String>,
    /// Declared PNG files encoded as base64, never filesystem paths or URLs.
    pub images: BTreeMap<String, String>,
}

impl Bundle {
    /// Controllers must validate before writing a received bundle to a local cache.
    pub fn valid(&self) -> bool {
        (self.entry.ends_with(".js") || self.entry.ends_with(".mjs"))
            && self.files.contains_key(&self.entry)
            && valid_paths(
                self.files
                    .keys()
                    .chain(self.images.keys())
                    .map(String::as_str),
            )
            && self.files.keys().all(|path| !path.ends_with(".png"))
            && self.images.iter().all(|(path, data)| {
                path.ends_with(".png")
                    && !data.is_empty()
                    && data.len() % 4 == 0
                    && data.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
                    })
            })
            && self
                .images
                .values()
                .try_fold(0usize, |size, data| size.checked_add(data.len()))
                .is_some_and(|size| size <= MAX_IMAGE_BYTES)
            && self
                .files
                .values()
                .try_fold(0usize, |size, text| size.checked_add(text.len()))
                .is_some_and(|size| size <= MAX_BYTES)
    }
}

pub(super) fn valid_paths<'a>(paths: impl Iterator<Item = &'a str>) -> bool {
    let mut names = std::collections::BTreeSet::new();
    for path in paths {
        if names.len() >= MAX_FILES
            || path.len() > 512
            || path.chars().any(char::is_control)
            || path.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
            || !names.insert(path.to_lowercase())
            || path.split('/').count() > 32
            || path.split('/').any(|part| {
                if part.is_empty() || part.ends_with(['.', ' ']) {
                    return true;
                }
                // A package from another OS must not become a Win32 device or alias.
                let stem = part
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(' ')
                    .to_ascii_uppercase();
                matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                    || ["COM", "LPT"].iter().any(|prefix| {
                        stem.strip_prefix(prefix).is_some_and(|suffix| {
                            matches!(
                                suffix,
                                "1" | "2"
                                    | "3"
                                    | "4"
                                    | "5"
                                    | "6"
                                    | "7"
                                    | "8"
                                    | "9"
                                    | "¹"
                                    | "²"
                                    | "³"
                            )
                        })
                    })
            })
        {
            return false;
        }
    }
    !names.is_empty()
}

#[cfg(test)]
mod tests;
