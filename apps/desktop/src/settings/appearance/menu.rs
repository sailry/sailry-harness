//! Theme card actions retain package identity across catalog changes.
use super::*;
use gpui_kit::component::native_menu::NativeMenu;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Command {
    Apply,
    Details,
    Homepage,
    Export,
    Uninstall,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = sailry_themes, no_json)]
pub(super) struct Dispatch {
    pub id: String,
    pub command: Command,
}

pub(super) fn definition(id: &str, cx: &App) -> Menu {
    let catalog = cx.global::<theme::Catalog>();
    let Some(index) = catalog.packages.iter().position(|package| package.id == id) else {
        return Menu::new("");
    };
    let package = &catalog.packages[index];
    let manifest = package.manifest.as_ref();
    let installed = manifest.is_some() && catalog.root.is_some();
    Menu::new(package.name.clone()).items([
        MenuItem::action(
            tr("appearance_select"),
            Dispatch {
                id: id.into(),
                command: Command::Apply,
            },
        )
        .checked(catalog.selected == index),
        MenuItem::action(
            tr("appearance_details"),
            Dispatch {
                id: id.into(),
                command: Command::Details,
            },
        )
        .disabled(manifest.is_none()),
        MenuItem::action(
            tr("appearance_homepage"),
            Dispatch {
                id: id.into(),
                command: Command::Homepage,
            },
        )
        .disabled(
            manifest
                .and_then(|manifest| manifest.homepage.as_ref())
                .is_none(),
        ),
        MenuItem::action(
            tr("appearance_export"),
            Dispatch {
                id: id.into(),
                command: Command::Export,
            },
        )
        .disabled(!installed),
        MenuItem::action(
            tr("appearance_uninstall"),
            Dispatch {
                id: id.into(),
                command: Command::Uninstall,
            },
        )
        .disabled(!installed || id == "builtin"),
    ])
}

pub(super) fn show(id: &str, position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    if !cfg!(any(target_os = "macos", target_os = "windows")) {
        crate::feedback::error("", &tr("workspace_native_menu_platform"), window, cx);
        return;
    }
    NativeMenu::from(definition(id, cx)).show(position, window, cx);
}

pub(super) fn apply(id: &str, window: &mut Window, cx: &mut App) {
    let index = cx
        .global::<theme::Catalog>()
        .packages
        .iter()
        .position(|package| package.id == id);
    if let Some(index) = index {
        theme::apply(index, Some(window), cx);
    } else {
        crate::feedback::error("", &tr("appearance_unavailable"), window, cx);
    }
}

impl Workspace {
    pub(super) fn theme_action(
        &mut self,
        dispatch: &Dispatch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let catalog = cx.global::<theme::Catalog>();
        let Some(package) = catalog
            .packages
            .iter()
            .find(|package| package.id == dispatch.id)
            .cloned()
        else {
            crate::feedback::error("", &tr("appearance_unavailable"), window, cx);
            return;
        };
        match dispatch.command {
            Command::Apply => apply(&package.id, window, cx),
            Command::Details => {
                if package.manifest.is_some() {
                    import::details(package, window, cx);
                }
            }
            Command::Homepage => {
                if let Some(url) = package.manifest.and_then(|manifest| manifest.homepage) {
                    cx.open_url(&url);
                }
            }
            Command::Export => export::choose(&package.id, window, cx),
            Command::Uninstall => {
                if package.id == "builtin" || package.manifest.is_none() || catalog.root.is_none() {
                    return;
                }
                crate::prompts::confirm(
                    &tr("appearance_uninstall"),
                    &rust_i18n::t!("appearance_remove", name = package.name.as_ref()),
                    tr("appearance_uninstall"),
                    window,
                    cx,
                    move |window, cx| match theme::remove(&package.id, window, cx) {
                        Ok(()) => crate::feedback::toast(
                            window,
                            tr("appearance_removed"),
                            notification::Notification::success(tr("appearance_removed")),
                            cx,
                        ),
                        Err(error) => {
                            eprintln!("could not uninstall theme: {error}");
                            crate::feedback::error("", &tr("appearance_remove_failed"), window, cx);
                        }
                    },
                );
            }
        }
    }
}
