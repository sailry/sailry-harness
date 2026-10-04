use super::package::Loaded;
use gpui_kit::{
    component::{theme::ThemeConfig, *},
    *,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, rc::Rc, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub package: String,
    pub mode: Mode,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            package: "builtin".into(),
            mode: Mode::System,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone)]
pub struct Package {
    pub id: String,
    pub name: SharedString,
    pub light: Rc<ThemeConfig>,
    pub dark: Rc<ThemeConfig>,
    pub manifest: Option<super::package::Manifest>,
    pub images: BTreeMap<String, Arc<RenderImage>>,
    pub incomplete: bool,
}

impl From<Loaded> for Package {
    fn from(loaded: Loaded) -> Self {
        for warning in &loaded.warnings {
            eprintln!("theme resource unavailable: {warning}");
        }
        Self::preview(&loaded)
    }
}

impl Package {
    pub fn preview(loaded: &Loaded) -> Self {
        Self {
            id: loaded.manifest.id.clone(),
            name: loaded.manifest.name.clone().into(),
            light: Rc::new(loaded.manifest.light.theme.clone()),
            dark: Rc::new(loaded.manifest.dark.theme.clone()),
            manifest: Some(loaded.manifest.clone()),
            images: loaded.images.clone(),
            incomplete: !loaded.warnings.is_empty(),
        }
    }
}

pub struct Catalog {
    pub packages: Vec<Package>,
    pub selected: usize,
    pub root: Option<PathBuf>,
    pub error: Option<&'static str>,
}
impl Global for Catalog {}

impl Catalog {
    pub fn new(cx: &App) -> Self {
        let builtin = cx
            .try_global::<Self>()
            .map(|catalog| catalog.packages[0].clone())
            .unwrap_or_else(|| Package {
                id: "builtin".into(),
                name: crate::tr("settings_theme_builtin"),
                light: cx.theme().light_theme.clone(),
                dark: cx.theme().dark_theme.clone(),
                manifest: None,
                images: BTreeMap::new(),
                incomplete: false,
            });
        let mut catalog = Self {
            packages: vec![builtin],
            selected: 0,
            root: cx
                .try_global::<crate::preferences::Preferences>()
                .and_then(|preferences| preferences.directory())
                .map(|path| path.join("themes")),
            error: None,
        };
        if let Some(root) = catalog.root.clone() {
            match std::fs::read_dir(root) {
                Ok(entries) => {
                    let mut paths = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry)
                                if super::package::valid_id(
                                    &entry.file_name().to_string_lossy(),
                                ) =>
                            {
                                paths.push(entry.path())
                            }
                            Ok(_) => {}
                            Err(error) => {
                                eprintln!("could not list themes: {error}");
                                catalog.error = Some("appearance_load_failed");
                            }
                        }
                    }
                    paths.sort();
                    for path in paths {
                        match Loaded::read(&path, false).and_then(|loaded| {
                            if path.file_name().and_then(|name| name.to_str())
                                != Some(loaded.manifest.id.as_str())
                            {
                                return Err(
                                    "installed theme ID does not match its directory".into()
                                );
                            }
                            catalog.add(loaded)
                        }) {
                            Ok(()) => {}
                            Err(error) => {
                                eprintln!("could not load theme: {error}");
                                catalog.error = Some("appearance_load_failed");
                            }
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    eprintln!("could not list themes: {error}");
                    catalog.error = Some("appearance_load_failed");
                }
            }
        }
        let selection = crate::preferences::data(cx).appearance;
        if let Some(index) = catalog
            .packages
            .iter()
            .position(|package| package.id == selection.package)
        {
            catalog.selected = index;
        } else {
            catalog.error = Some("appearance_restore_failed");
        }
        catalog
    }

    pub fn check(&self, loaded: &Loaded) -> Result<(), String> {
        if self
            .packages
            .iter()
            .any(|package| package.id == loaded.manifest.id)
        {
            return Err("theme package is already installed".into());
        }
        let allocation = |image: &Arc<RenderImage>| {
            let size = image.size(0);
            u64::from(size.width) * u64::from(size.height) * 4
        };
        let used: u64 = self
            .packages
            .iter()
            .flat_map(|package| package.images.values())
            .map(allocation)
            .sum();
        let added: u64 = loaded.images.values().map(allocation).sum();
        if self.packages.len() >= 33 || used + added > 128 * 1024 * 1024 {
            return Err("installed themes exceed resource limit".into());
        }
        Ok(())
    }

    pub fn add(&mut self, loaded: Loaded) -> Result<(), String> {
        self.check(&loaded)?;
        self.packages.push(loaded.into());
        Ok(())
    }
}

pub fn apply(index: usize, window: Option<&mut Window>, cx: &mut App) {
    let package = cx.global::<Catalog>().packages[index].clone();
    cx.update_global::<Catalog, _>(|catalog, _| catalog.selected = index);
    crate::preferences::update(cx, |data| data.appearance.package = package.id.clone());
    apply_colors(package, window, cx);
}

pub fn apply_colors(package: Package, window: Option<&mut Window>, cx: &mut App) {
    let mode = cx.theme().mode;
    let builtin = cx.global::<Catalog>().packages[0].clone();
    let complete = |mut config: Rc<ThemeConfig>, defaults: Rc<ThemeConfig>| {
        let config_mut = Rc::make_mut(&mut config);
        // A package overlays the configured Kit defaults. Kit's fallback for a
        // missing primary foreground is the body foreground, which can disappear
        // against the default primary button background in a partial color theme.
        let mut colors = serde_json::to_value(&defaults.colors).expect("Kit color serialization");
        let overrides = serde_json::to_value(&config_mut.colors).expect("Kit color serialization");
        for (key, value) in overrides.as_object().expect("Kit color object") {
            if !value.is_null() {
                colors[key] = value.clone();
            }
        }
        config_mut.colors = serde_json::from_value(colors).expect("Kit color configuration");
        config_mut.font_size = config_mut.font_size.or(defaults.font_size);
        config_mut.font_family = config_mut
            .font_family
            .clone()
            .or_else(|| defaults.font_family.clone());
        config_mut.mono_font_size = config_mut.mono_font_size.or(defaults.mono_font_size);
        config_mut.mono_font_family = config_mut
            .mono_font_family
            .clone()
            .or_else(|| defaults.mono_font_family.clone());
        config_mut.radius = config_mut.radius.or(defaults.radius);
        config_mut.radius_lg = config_mut.radius_lg.or(defaults.radius_lg);
        config_mut.shadow = config_mut.shadow.or(defaults.shadow);
        config_mut.highlight = defaults.highlight.clone();
        config
    };
    let theme = Theme::global_mut(cx);
    theme.light_theme = complete(package.light, builtin.light);
    theme.dark_theme = complete(package.dark, builtin.dark);
    Theme::change(mode, window, cx);
}

pub fn remove(id: &str, window: &mut Window, cx: &mut App) -> Result<(), String> {
    let catalog = cx.global::<Catalog>();
    let index = catalog
        .packages
        .iter()
        .position(|package| package.id == id)
        .filter(|index| *index > 0)
        .ok_or("theme package is unavailable")?;
    if let Some(root) = &catalog.root {
        let path = root.join(id);
        // IDs come from validated catalog entries, and only the owned package is removed.
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("installed theme directory changed".into());
        }
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    if catalog.selected == index {
        apply(0, Some(window), cx);
    }
    cx.update_global::<Catalog, _>(|catalog, _| {
        catalog.packages.remove(index);
        if catalog.selected > index {
            catalog.selected -= 1;
        }
        catalog.error = None;
    });
    cx.refresh_windows();
    Ok(())
}
