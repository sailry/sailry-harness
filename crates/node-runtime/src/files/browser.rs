//! Host picker behavior follows Sailry Code d9b56405, harbor_file_picker.dart.
use super::{Control, directory, io_error};
use sailry_protocol::{file_browser::*, *};
use std::path::{Path, PathBuf};

mod actions;
pub(crate) use actions::{execute, targets};

pub(super) fn list(
    selected: Option<&str>,
    after: Option<&DirectoryCursor>,
    control: &Control,
) -> Result<Listing, Fault> {
    let locations = locations();
    let path = selected
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .or_else(|| {
            locations
                .first()
                .map(|location| PathBuf::from(&location.path))
        })
        .ok_or_else(|| invalid("no filesystem locations are available"))?;
    let root = resolve(&path)?;
    let mut directory = directory::list(&root, String::new(), after, control)?;
    directory.path = text(&root)?;
    Ok(Listing {
        separator: std::path::MAIN_SEPARATOR,
        locations,
        parent: root.parent().map(text).transpose()?,
        directory,
    })
}

fn locations() -> Vec<Location> {
    let mut locations = Vec::new();
    for (kind, path) in [
        (LocationKind::Home, dirs::home_dir()),
        (LocationKind::Desktop, dirs::desktop_dir()),
        (LocationKind::Documents, dirs::document_dir()),
        (LocationKind::Downloads, dirs::download_dir()),
    ] {
        if let Some(path) = path.filter(|path| path.is_dir())
            && let Ok(path) = text(&path)
        {
            locations.push(Location {
                kind,
                name: String::new(),
                path,
            });
        }
    }
    for disk in sysinfo::Disks::new_with_refreshed_list().list() {
        if let Ok(path) = text(disk.mount_point())
            && !locations.iter().any(|location| location.path == path)
        {
            locations.push(Location {
                kind: LocationKind::Volume,
                name: disk.name().to_string_lossy().into_owned(),
                path,
            });
        }
    }
    if let Some(home) = dirs::home_dir()
        && let Some(root) = home.ancestors().last()
        && let Ok(path) = text(root)
        && !locations.iter().any(|location| location.path == path)
    {
        locations.push(Location {
            kind: LocationKind::Root,
            name: String::new(),
            path,
        });
    }
    locations
}

fn resolve(path: &Path) -> Result<PathBuf, Fault> {
    if !path.is_absolute() {
        return Err(invalid("an absolute path is required"));
    }
    path.canonicalize().map_err(io_error)
}

fn text(path: &Path) -> Result<String, Fault> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("path is not UTF-8"))
}

fn invalid(message: &str) -> Fault {
    Fault::new(ErrorCode::InvalidRequest, message)
}
