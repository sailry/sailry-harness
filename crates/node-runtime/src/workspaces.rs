//! Exact layouts for Node-owned execution workspaces, separate from protected storage.
use sailry_protocol::{
    DatabaseId, ErrorCode, Fault, SessionId, SshId, WorktreeId, connection::Resource,
    plugin::conversation::Binding,
};
use std::path::{Path, PathBuf};

pub(crate) fn worktrees(root: &Path) -> PathBuf {
    root.join(".sailry").join("worktrees")
}

pub(crate) fn session(profile: &Path, id: SessionId) -> PathBuf {
    profile
        .join("workspaces")
        .join("sessions")
        .join(id.to_string())
}

pub(crate) fn connection(profile: &Path, resource: Resource) -> PathBuf {
    let (kind, id) = match resource {
        Resource::Database(id) => ("database", id.to_string()),
        Resource::Ssh(id) => ("ssh", id.to_string()),
    };
    profile
        .join("connections")
        .join(kind)
        .join(id)
        .join("workspace")
}

pub(crate) fn assistant(profile: &Path, binding: &Binding) -> Result<PathBuf, Fault> {
    let identity = serde_json::to_vec(&(&binding.package.name, &binding.id)).map_err(|_| {
        Fault::new(
            ErrorCode::Internal,
            "Node workspace identity is unavailable",
        )
    })?;
    Ok(profile
        .join("assistants")
        .join(blake3::hash(&identity).to_hex().as_str())
        .join("workspace"))
}

/// Launch owners resolve the registered identity first; the Store also checks ownership.
pub(crate) fn managed(root: &Path, profile: &Path) -> bool {
    if !root.is_absolute() || !profile.is_absolute() {
        return false;
    }
    let Ok(relative) = root.strip_prefix(profile) else {
        return false;
    };
    let Some(parts) = relative
        .iter()
        .map(|part| part.to_str())
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let expected = match parts.as_slice() {
        ["worktrees", name] => name
            .parse::<WorktreeId>()
            .ok()
            .map(|id| profile.join("worktrees").join(id.to_string())),
        [".sailry", "worktrees", name] => name
            .parse::<WorktreeId>()
            .ok()
            .map(|id| worktrees(profile).join(id.to_string())),
        ["workspaces", "sessions", name] => name
            .parse::<SessionId>()
            .ok()
            .map(|id| session(profile, id)),
        ["connections", "database", name, "workspace"] => name
            .parse::<DatabaseId>()
            .ok()
            .map(|id| connection(profile, Resource::Database(id))),
        ["connections", "ssh", name, "workspace"] => name
            .parse::<SshId>()
            .ok()
            .map(|id| connection(profile, Resource::Ssh(id))),
        ["assistants", name, "workspace"] => blake3::Hash::from_hex(name).ok().map(|digest| {
            profile
                .join("assistants")
                .join(digest.to_hex().as_str())
                .join("workspace")
        }),
        _ => None,
    };
    expected.is_some_and(|expected| expected.as_os_str() == root.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sailry_protocol::plugin::Reference;

    fn binding() -> Binding {
        Binding {
            package: Reference {
                name: "fixture-assistant".into(),
                digest: blake3::hash(b"first").to_hex().to_string(),
                settings_revision: 1,
            },
            id: "notes".into(),
        }
    }

    #[test]
    fn preserves_assistant_identity() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().canonicalize().unwrap();
        let first = binding();
        let root = assistant(&profile, &first).unwrap();
        let identity = serde_json::to_vec(&(&first.package.name, &first.id)).unwrap();
        assert_eq!(
            root,
            profile
                .join("assistants")
                .join(blake3::hash(&identity).to_hex().as_str())
                .join("workspace")
        );
        let mut revised = first.clone();
        revised.package.digest = blake3::hash(b"second").to_hex().to_string();
        revised.package.settings_revision = 2;
        assert_eq!(assistant(&profile, &revised).unwrap(), root);
        revised.id = "other".into();
        assert_ne!(assistant(&profile, &revised).unwrap(), root);
        revised = first;
        revised.package.name = "other-assistant".into();
        assert_ne!(assistant(&profile, &revised).unwrap(), root);
    }

    #[test]
    fn confines_exact_roots() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().canonicalize().unwrap();
        let roots = [
            worktrees(&profile).join(WorktreeId::new().to_string()),
            profile
                .join("worktrees")
                .join(WorktreeId::new().to_string()),
            session(&profile, SessionId::new()),
            connection(&profile, Resource::Database(DatabaseId::new())),
            connection(&profile, Resource::Ssh(SshId::new())),
            assistant(&profile, &binding()).unwrap(),
        ];
        for root in roots {
            assert!(managed(&root, &profile), "{root:?}");
            assert!(!managed(&root.join("nested"), &profile));
            assert!(!managed(root.parent().unwrap(), &profile));
        }
        for relative in [
            "storage",
            "plugins/data",
            "tmp",
            "worktrees/invalid",
            ".sailry/worktrees/invalid",
            ".sailry/worktrees/00000000-0000-0000-0000-000000000000",
            ".sailry/worktrees/0ED44566-789B-4ABC-9ABC-DEF123456789",
            "worktrees/00000000-0000-0000-0000-000000000000",
            "workspaces/sessions/00000000-0000-0000-0000-000000000000",
            "connections/ssh/00000000-0000-0000-0000-000000000000/workspace",
            "connections/database/00000000-0000-0000-0000-000000000000/workspace",
            "assistants/invalid/workspace",
            "worktrees/0ED44566-789B-4ABC-9ABC-DEF123456789",
            "workspaces/sessions/0ED44566-789B-4ABC-9ABC-DEF123456789",
            "connections/ssh/0ED44566-789B-4ABC-9ABC-DEF123456789/workspace",
            "connections/database/0ED44566-789B-4ABC-9ABC-DEF123456789/workspace",
            "connections/other/0ed44566-789b-4abc-9abc-def123456789/workspace",
        ] {
            assert!(!managed(&profile.join(relative), &profile), "{relative}");
        }
        let digest = blake3::hash(b"identity")
            .to_hex()
            .to_string()
            .to_uppercase();
        assert!(!managed(
            &profile.join("assistants").join(digest).join("workspace"),
            &profile
        ));
        let id = WorktreeId::new();
        let separator = std::path::MAIN_SEPARATOR;
        for alias in [
            format!("{separator}worktrees{separator}.{separator}{id}"),
            format!("{separator}worktrees{separator}{separator}{id}"),
        ] {
            let mut raw = profile.as_os_str().to_os_string();
            raw.push(alias);
            assert!(!managed(&PathBuf::from(raw), &profile));
        }
        #[cfg(windows)]
        {
            let drive = Path::new(r"C:\node");
            let id = SessionId::new();
            let expected = PathBuf::from(format!(r"C:\node\workspaces\sessions\{id}"));
            assert_eq!(session(drive, id).as_os_str(), expected.as_os_str());
            assert!(managed(&expected, drive));
        }
        assert!(!managed(&profile, &profile));
        assert!(!managed(&profile.join("worktrees"), &profile));
        assert!(!managed(Path::new("worktrees"), Path::new("")));
    }
}
