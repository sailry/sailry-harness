//! Local controller preferences; never sent to an execution Node.
use gpui_kit::{App, BorrowAppContext, Global};
use serde::{Deserialize, Serialize};
mod composer;
mod dictation;
pub(crate) mod plugins;
pub(crate) mod recent;
pub(crate) mod sessions;
pub(crate) use composer::{Composer, Model};
pub(crate) use dictation::{Dictation, Language};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Data {
    pub version: u32,
    pub notifications: [bool; 4],
    pub toast_seconds: u64,
    pub pairing: String,
    pub devices: Option<std::collections::BTreeMap<String, Device>>,
    pub terminal: Terminal,
    pub appearance: crate::theme::Selection,
    pub composer: Option<Composer>,
    pub workspaces: Option<serde_json::Value>,
    pub feature_pins: Option<std::collections::BTreeMap<String, bool>>,
    pub feature_order: Option<Vec<String>>,
    pub browser_persistent: Option<bool>,
    pub surfaces: Option<Surfaces>,
    pub shortcuts: Option<std::collections::BTreeMap<String, String>>,
    pub dictation: Option<Dictation>,
    pub recent: Option<Vec<recent::Visit>>,
    pub sessions: Option<std::collections::BTreeMap<String, sessions::State>>,
    pub plugin_directories: Option<std::collections::BTreeMap<String, plugins::Directory>>,
    pub message_display: Option<MessageDisplay>,
    pub language: Option<crate::locale::Language>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageDisplay {
    #[default]
    Detailed,
    Compact,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Surfaces {
    pub main: f32,
    pub sidebar: Option<f32>,
}

impl Default for Surfaces {
    fn default() -> Self {
        Self {
            main: 83.5,
            sidebar: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Device {
    pub name: Option<String>,
    pub execution: bool,
    pub platform: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Terminal {
    pub font_size: u8,
    pub font_family: String,
    pub paste_protection: bool,
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            font_size: 14,
            font_family: String::new(),
            paste_protection: true,
        }
    }
}

impl Default for Data {
    fn default() -> Self {
        Self {
            version: 1,
            notifications: [true; 4],
            toast_seconds: 5,
            pairing: String::new(),
            devices: None,
            terminal: Terminal::default(),
            appearance: crate::theme::Selection::default(),
            composer: None,
            workspaces: None,
            feature_pins: None,
            feature_order: None,
            browser_persistent: None,
            surfaces: None,
            shortcuts: None,
            dictation: None,
            sessions: None,
            plugin_directories: None,
            recent: None,
            message_display: None,
            language: None,
        }
    }
}

impl Data {
    fn validate(&self) -> Result<(), String> {
        if self.surfaces.as_ref().is_some_and(|s| {
            !(45.0..=100.0).contains(&s.main)
                || s.sidebar
                    .is_some_and(|opacity| !(45.0..=100.0).contains(&opacity))
        }) {
            return Err("invalid panel opacity".into());
        }
        if self.version != 1 {
            return Err("unsupported desktop preferences version".into());
        }
        // Workspace restoration validates its opaque payload independently so an
        // unreadable layout does not discard unrelated desktop preferences.
        if !crate::theme::package::valid_id(&self.appearance.package) {
            return Err("invalid theme selection".into());
        }
        if ![3, 5, 8, 10].contains(&self.toast_seconds) {
            return Err("invalid notification duration".into());
        }
        if !(8..=32).contains(&self.terminal.font_size)
            || self.terminal.font_family.chars().any(char::is_control)
        {
            return Err("invalid terminal display preferences".into());
        }
        if !self.pairing.is_empty() {
            sailry_link::rendezvous::Relay::new(&self.pairing)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct Preferences {
    pub data: Data,
    path: Option<PathBuf>,
    readable: bool,
    pub error: Option<&'static str>,
}
impl Global for Preferences {}

impl Preferences {
    pub fn directory(&self) -> Option<&Path> {
        self.path.as_ref().and_then(|path| path.parent())
    }

    pub fn open(path: PathBuf) -> Self {
        let result = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Data>(&bytes)
                .map_err(|error| error.to_string())
                .and_then(|data| {
                    data.validate()?;
                    Ok(data)
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Data::default()),
            Err(error) => Err(error.to_string()),
        };
        match result {
            Ok(data) => Self {
                data,
                path: Some(path),
                readable: true,
                error: None,
            },
            Err(error) => {
                eprintln!("could not read desktop preferences: {error}");
                Self {
                    data: Data::default(),
                    path: Some(path),
                    readable: false,
                    error: Some("preferences_read_failed"),
                }
            }
        }
    }

    fn save(&mut self) {
        let Some(path) = &self.path else {
            self.error = None;
            return;
        };
        if !self.readable {
            return;
        }
        self.error = match save(path, &self.data) {
            Ok(()) => None,
            Err(error) => {
                eprintln!("could not save desktop preferences: {error}");
                Some("preferences_save_failed")
            }
        };
    }
}

fn save(path: &Path, data: &Data) -> Result<(), Box<dyn std::error::Error>> {
    data.validate()?;
    let parent = path
        .parent()
        .ok_or("desktop preferences directory is missing")?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec_pretty(data)?)?;
    file.persist(path)?;
    Ok(())
}

pub(crate) fn init(cx: &mut App) {
    if cx.try_global::<Preferences>().is_none() {
        cx.set_global(Preferences::default());
    }
}
pub(crate) fn data(cx: &App) -> Data {
    cx.try_global::<Preferences>()
        .map(|preferences| preferences.data.clone())
        .unwrap_or_default()
}
pub(crate) fn update(cx: &mut App, change: impl FnOnce(&mut Data)) {
    init(cx);
    cx.update_global::<Preferences, _>(|preferences, _| {
        change(&mut preferences.data);
        preferences.save();
    });
}
