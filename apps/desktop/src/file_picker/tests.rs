use super::*;
use core::prelude::v1::test;

mod selection {
    use super::*;
    fn listing(separator: char) -> Listing {
        Listing {
            separator,
            locations: Vec::new(),
            parent: None,
            directory: sailry_protocol::Directory {
                path: if separator == '/' {
                    "/node/目录"
                } else {
                    "D:\\node\\目录"
                }
                .into(),
                entries: vec![
                    FileEntry {
                        name: "folder".into(),
                        kind: EntryKind::Directory,
                        size: 0,
                    },
                    FileEntry {
                        name: "Alpha.md".into(),
                        kind: EntryKind::File,
                        size: 1,
                    },
                    FileEntry {
                        name: ".hidden".into(),
                        kind: EntryKind::File,
                        size: 1,
                    },
                    FileEntry {
                        name: "second.txt".into(),
                        kind: EntryKind::File,
                        size: 1,
                    },
                ],
                truncated: false,
                unsupported_names: 0,
                revision: "fixture".into(),
                next: None,
            },
        }
    }
    #[test]
    fn accepted_targets() {
        for (scenario, check) in [
            (
                "filters_keep_source_indices",
                (|_scenario: &str| {
                    let listing = listing('/');
                    assert_eq!(
                        super::super::selection::visible(
                            &listing.directory.entries,
                            false,
                            "ALPHA"
                        ),
                        vec![1],
                        "filters keep source indices"
                    );
                    assert_eq!(
                        super::super::selection::visible(&listing.directory.entries, false, ""),
                        vec![0, 1, 3],
                        "filters keep source indices"
                    );
                    assert_eq!(
                        super::super::selection::visible(
                            &listing.directory.entries,
                            true,
                            "hidden"
                        ),
                        vec![2],
                        "filters keep source indices"
                    );
                }) as fn(&str),
            ),
            (
                "directory_requires_folder_selection",
                (|_scenario: &str| {
                    let listing = listing('/');
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Directory,
                            &listing,
                            &BTreeSet::new(),
                            ""
                        ),
                        Some(Selection::Directory("/node/目录".into())),
                        "directory requires folder selection"
                    );
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Directory,
                            &listing,
                            &BTreeSet::from([1]),
                            ""
                        ),
                        None,
                        "directory requires folder selection"
                    );
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Directory,
                            &listing,
                            &BTreeSet::from([0]),
                            ""
                        ),
                        Some(Selection::Directory("/node/目录/folder".into())),
                        "directory requires folder selection"
                    );
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Directory,
                            &listing,
                            &BTreeSet::from([0, 1]),
                            ""
                        ),
                        None,
                        "directory requires folder selection"
                    );
                }),
            ),
            (
                "files_require_regular_selection",
                (|_scenario: &str| {
                    let listing = listing('/');
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Files { multiple: false },
                            &listing,
                            &BTreeSet::from([0, 1]),
                            ""
                        ),
                        None,
                        "files require regular selection"
                    );
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Files { multiple: false },
                            &listing,
                            &BTreeSet::from([0]),
                            ""
                        ),
                        None,
                        "files require regular selection"
                    );
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Files { multiple: false },
                            &listing,
                            &BTreeSet::from([1, 3]),
                            ""
                        ),
                        None,
                        "files require regular selection"
                    );
                    assert_eq!(
                        super::super::selection::resolve(
                            &Mode::Files { multiple: true },
                            &listing,
                            &BTreeSet::from([0, 1, 3]),
                            ""
                        ),
                        Some(Selection::Files(vec![
                            "/node/目录/Alpha.md".into(),
                            "/node/目录/second.txt".into()
                        ])),
                        "files require regular selection"
                    );
                }),
            ),
            (
                "save_uses_node_separator",
                (|_scenario: &str| {
                    let mode = Mode::Save {
                        suggested_name: String::new(),
                    };
                    for separator in ['/', '\\'] {
                        let listing = listing(separator);
                        assert_eq!(
                            super::super::selection::resolve(
                                &mode,
                                &listing,
                                &BTreeSet::new(),
                                "结果.md"
                            ),
                            Some(Selection::Save(format!(
                                "{}{separator}结果.md",
                                listing.directory.path
                            ))),
                            "save uses node separator"
                        );
                        for name in ["", ".", "..", "parent/file", "parent\\file", "bad\0name"] {
                            assert_eq!(
                                super::super::selection::resolve(
                                    &mode,
                                    &listing,
                                    &BTreeSet::new(),
                                    name
                                ),
                                None,
                                "save uses node separator"
                            );
                        }
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
