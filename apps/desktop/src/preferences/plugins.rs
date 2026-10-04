//! Controller-local plugin directory hints, scoped to a Node and package digest.
use super::*;
use sailry_protocol::NodeId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Directory {
    digest: String,
    install_revision: u64,
    path: PathBuf,
}

fn key(node: NodeId, name: &str) -> String {
    let node: String = node.0.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{node}/{name}")
}

pub(crate) fn directory(node: NodeId, name: &str, digest: &str, cx: &App) -> Option<PathBuf> {
    cx.try_global::<Preferences>()
        .and_then(|preferences| {
            preferences
                .data
                .plugin_directories
                .as_ref()?
                .get(&key(node, name))
        })
        .filter(|directory| directory.digest == digest)
        .map(|directory| directory.path.clone())
}

pub(crate) fn remember(
    node: NodeId,
    name: &str,
    digest: &str,
    install_revision: u64,
    path: PathBuf,
    cx: &mut App,
) {
    let key = key(node, name);
    if cx
        .try_global::<Preferences>()
        .and_then(|preferences| preferences.data.plugin_directories.as_ref()?.get(&key))
        .is_some_and(|directory| {
            directory.install_revision > install_revision
                || (directory.install_revision == install_revision
                    && directory.digest == digest
                    && directory.path == path)
        })
    {
        return;
    }
    super::update(cx, |data| {
        data.plugin_directories.get_or_insert_default().insert(
            key,
            Directory {
                digest: digest.into(),
                install_revision,
                path,
            },
        );
    });
}

pub(crate) fn forget(node: NodeId, name: &str, through_revision: u64, cx: &mut App) {
    let key = key(node, name);
    if !cx
        .try_global::<Preferences>()
        .and_then(|preferences| preferences.data.plugin_directories.as_ref()?.get(&key))
        .is_some_and(|directory| directory.install_revision <= through_revision)
    {
        return;
    }
    super::update(cx, |data| {
        if let Some(directories) = &mut data.plugin_directories {
            directories.remove(&key);
        }
    });
}
