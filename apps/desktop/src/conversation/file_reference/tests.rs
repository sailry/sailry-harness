use super::*;
mod service;

fn roots() -> [Worktree; 3] {
    let project = sailry_protocol::ProjectId::new();
    [
        Worktree {
            id: WorktreeId::new(),
            project: None,
            path: "/node/workspaces/sessions/original".into(),
            main: false,
        },
        Worktree {
            id: WorktreeId::new(),
            project: Some(project),
            path: "/projects/themes".into(),
            main: true,
        },
        Worktree {
            id: WorktreeId::new(),
            project: Some(project),
            path: "/projects/themes/.sailry/worktrees/checkout".into(),
            main: false,
        },
    ]
}

#[test]
fn locates_registered_absolute_paths() {
    let roots = roots();
    for (link, owner) in [
        ("/projects/themes/THEME_DEVELOPMENT_SPEC.md#L7", 1),
        ("file:///projects/themes/THEME_DEVELOPMENT_SPEC.md:7", 1),
        (
            "/projects/themes/.sailry/worktrees/checkout/THEME_DEVELOPMENT_SPEC.md#L7",
            2,
        ),
    ] {
        assert_eq!(
            locate(link, roots[0].id, &roots),
            Some((
                roots[owner].id,
                Target::Workspace {
                    path: "THEME_DEVELOPMENT_SPEC.md".into(),
                    line: Some(7),
                }
            )),
            "{link}"
        );
    }
}

#[test]
fn keeps_relative_paths_at_their_origin() {
    let roots = roots();
    for origin in [&roots[0], &roots[1]] {
        assert_eq!(
            locate("THEME_DEVELOPMENT_SPEC.md#L3", origin.id, &roots),
            Some((
                origin.id,
                Target::Workspace {
                    path: "THEME_DEVELOPMENT_SPEC.md".into(),
                    line: Some(3),
                }
            ))
        );
    }
    for path in [
        "/projects/themes-other/private.md",
        "file:///unregistered/private.md",
        "../private.md",
    ] {
        assert!(
            matches!(locate(path, roots[0].id, &roots), Some((owner, Target::External(_))) if owner == roots[0].id)
        );
    }
}

#[test]
fn requires_the_registered_origin() {
    let roots = roots();
    assert_eq!(
        locate("/projects/themes/file.md", WorktreeId::new(), &roots),
        None
    );
    for path in [
        "https://example.test/file.md",
        "file://other-node/path",
        "file.md%00",
    ] {
        assert_eq!(locate(path, roots[0].id, &roots), None, "{path}");
    }
}
#[test]
fn resolves_worktree_lines() {
    assert_eq!(parse("src/main.rs#L42"), Some(("src/main.rs", Some(42))));
    assert_eq!(parse("./src/main.rs:42"), Some(("src/main.rs", Some(42))));
    assert_eq!(
        parse("docs/build notes.md#L12"),
        Some(("docs/build notes.md", Some(12)))
    );
    assert_eq!(parse("README.md#setup"), Some(("README.md", None)));
}
#[test]
fn rejects_invalid_targets() {
    for path in [
        "a.rs:0",
        "a.rs#L-1",
        "a.rs#L999999999999999",
        "a.rs:3#L5",
        "../secret",
        "/etc/passwd",
        "C:\\secret",
        "file:///etc/passwd",
        "https://example.test",
    ] {
        assert_eq!(parse(path), None, "{path}");
    }
}

#[test]
fn resolves_urls() {
    for link in [
        "build%20notes.md#L3",
        "/project/build notes.md:3",
        "file:///project/build%20notes.md#L3",
    ] {
        assert_eq!(
            resolve(link, "/project"),
            Some(Target::Workspace {
                path: "build notes.md".into(),
                line: Some(3)
            })
        );
    }
    for link in [
        "../outside.png",
        "%2e%2e/outside.png",
        "file:///outside.png",
    ] {
        assert_eq!(
            resolve(link, "/project"),
            Some(Target::External("/outside.png".into()))
        );
    }
    for link in [
        "https://example.test/a.png",
        "file://remote/path",
        "image.png#L0",
        "image.png%00",
        "a.rs:3#L5",
    ] {
        assert_eq!(resolve(link, "/project"), None, "{link}");
    }
}
