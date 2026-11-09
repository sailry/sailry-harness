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
fn scoped_targets() {
    for (scenario, check) in [
        (
            "locates_registered_absolute_paths",
            (|_scenario: &str| {
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
                        "locates registered absolute paths: {link}"
                    );
                }
            }) as fn(&str),
        ),
        (
            "keeps_relative_paths_at_their_origin",
            (|_scenario: &str| {
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
                        )),
                        "keeps relative paths at their origin"
                    );
                }
                for path in [
                    "/projects/themes-other/private.md",
                    "file:///unregistered/private.md",
                    "../private.md",
                ] {
                    assert!(
                        matches!(locate(path, roots[0].id, &roots), Some((owner, Target::External(_))) if owner == roots[0].id),
                        "keeps relative paths at their origin"
                    );
                }
            }),
        ),
        (
            "requires_the_registered_origin",
            (|_scenario: &str| {
                let roots = roots();
                assert_eq!(
                    locate("/projects/themes/file.md", WorktreeId::new(), &roots),
                    None,
                    "requires the registered origin"
                );
                for path in [
                    "https://example.test/file.md",
                    "file://other-node/path",
                    "file.md%00",
                ] {
                    assert_eq!(
                        locate(path, roots[0].id, &roots),
                        None,
                        "requires the registered origin: {path}"
                    );
                }
            }),
        ),
        (
            "resolves_worktree_lines",
            (|_scenario: &str| {
                assert_eq!(
                    parse("src/main.rs#L42"),
                    Some(("src/main.rs", Some(42))),
                    "resolves worktree lines"
                );
                assert_eq!(
                    parse("./src/main.rs:42"),
                    Some(("src/main.rs", Some(42))),
                    "resolves worktree lines"
                );
                assert_eq!(
                    parse("docs/build notes.md#L12"),
                    Some(("docs/build notes.md", Some(12))),
                    "resolves worktree lines"
                );
                assert_eq!(
                    parse("README.md#setup"),
                    Some(("README.md", None)),
                    "resolves worktree lines"
                );
            }),
        ),
        (
            "recovers_editor_line_forms",
            (|_scenario: &str| {
                for (link, line) in [
                    ("src/main.rs:42:7", Some(42)),
                    ("src/main.rs#L42-L50", Some(42)),
                    ("src/main.rs#L42-50", Some(42)),
                    ("src/main.rs#L42C7", Some(42)),
                    ("README.md#Licensing", None),
                ] {
                    let path = link.split([':', '#']).next().unwrap();
                    assert_eq!(parse(link), Some((path, line)), "{link}");
                }
                assert_eq!(
                    resolve("/project/src/main.rs:42:7", "/project"),
                    Some(Target::Workspace {
                        path: "src/main.rs".into(),
                        line: Some(42)
                    }),
                );
                for link in ["a.rs#L42x", "a.rs:3:4#L5"] {
                    assert_eq!(parse(link), None, "{link}");
                }
            }),
        ),
        (
            "rejects_invalid_targets",
            (|_scenario: &str| {
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
                    assert_eq!(parse(path), None, "rejects invalid targets: {path}");
                }
            }),
        ),
        (
            "resolves_windows_execution_paths",
            (|_scenario: &str| {
                for root in ["C:/project", r"\\?\C:\project"] {
                    for link in [
                        "build%20notes.md#L3",
                        "C:/project/build notes.md:3",
                        r"C:\project\build notes.md#L3",
                        r"\\?\C:\project\build notes.md#L3",
                        "file:///C:/project/build%20notes.md#L3",
                        "file:///c:/project/build%20notes.md#L3",
                    ] {
                        assert_eq!(
                            resolve(link, root),
                            Some(Target::Workspace {
                                path: "build notes.md".into(),
                                line: Some(3)
                            }),
                            "{root}: {link}"
                        );
                    }
                    for (link, path) in [
                        ("../outside.png", "C:/outside.png"),
                        ("C:/project-other/file.md", "C:/project-other/file.md"),
                        ("D:/outside.png", "D:/outside.png"),
                    ] {
                        assert_eq!(
                            resolve(link, root),
                            Some(Target::External(path.into())),
                            "{root}: {link}"
                        );
                    }
                }
            }),
        ),
        (
            "resolves_unc_execution_paths",
            (|_scenario: &str| {
                for root in [r"\\server\share\project", r"\\?\UNC\server\share\project"] {
                    for link in [
                        "build%20notes.md#L3",
                        r"\\server\share\project\build notes.md#L3",
                        r"\\?\UNC\server\share\project\build notes.md#L3",
                    ] {
                        assert_eq!(
                            resolve(link, root),
                            Some(Target::Workspace {
                                path: "build notes.md".into(),
                                line: Some(3)
                            }),
                            "{root}: {link}"
                        );
                    }
                    assert_eq!(resolve("file://server/share/project/file.md", root), None);
                }
            }),
        ),
        (
            "normalizes_decoded_parent_paths",
            (|_scenario: &str| {
                for root in ["/project", "/project/./", "/project/../project"] {
                    for link in [
                        "%2e%2e%2foutside.png",
                        "%2e%2e%5coutside.png",
                        "nested%2f..%2f..%2foutside.png",
                    ] {
                        assert_eq!(
                            resolve(link, root),
                            Some(Target::External("/outside.png".into())),
                            "{root}: {link}"
                        );
                    }
                    assert_eq!(
                        resolve("nested%2f..%2ffile.md", root),
                        Some(Target::Workspace {
                            path: "file.md".into(),
                            line: None
                        }),
                        "{root}"
                    );
                }
                assert_eq!(
                    resolve("file.md", "/project%2fnotes"),
                    Some(Target::Workspace {
                        path: "file.md".into(),
                        line: None
                    })
                );
            }),
        ),
        (
            "resolves_urls",
            (|_scenario: &str| {
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
                        }),
                        "resolves urls"
                    );
                }
                for link in [
                    "../outside.png",
                    "%2e%2e/outside.png",
                    "file:///outside.png",
                ] {
                    assert_eq!(
                        resolve(link, "/project"),
                        Some(Target::External("/outside.png".into())),
                        "resolves urls"
                    );
                }
                for link in [
                    "https://example.test/a.png",
                    "file://remote/path",
                    "image.png#L0",
                    "image.png%00",
                    "a.rs:3#L5",
                ] {
                    assert_eq!(resolve(link, "/project"), None, "resolves urls: {link}");
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
