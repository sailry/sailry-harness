use super::*;
use crate::files::{Access, changes, edit};
use sailry_protocol::{ChangeKind, ChangeStatus, ChangesStatus, FileChanges};
use std::path::Path;

fn revision(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn reason(input: &str) -> String {
    let error = parse(input).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest, "{error:?}");
    error.message
}

fn chunks(body: &str) -> Vec<Chunk> {
    let input = format!("*** Begin Patch\n*** Update File: file.txt\n{body}*** End Patch\n");
    let Hunk::Update { chunks, .. } = parse(&input).unwrap().remove(0) else {
        panic!("update expected")
    };
    chunks
}

fn patched(before: &str, body: &str) -> Result<String, Fault> {
    update::apply(before, &chunks(body)).map(|update| update.after)
}

mod parsing {
    use super::*;

    #[test]
    fn reads_each_directive() {
        let hunks = parse(
            "*** Begin Patch
*** Add File: src/new.rs
+pub fn new() {}
+
*** Delete File: src/dead.rs
*** Update File: src/lib.rs
*** Move to: src/core.rs
@@ fn main() {
 let a = 1;
-let b = 2;
+let b = 3;
@@
-tail
+end
*** End of File
*** End Patch
",
        )
        .unwrap();
        assert_eq!(
            hunks,
            [
                Hunk::Add {
                    path: "src/new.rs".into(),
                    contents: "pub fn new() {}\n\n".into(),
                },
                Hunk::Delete {
                    path: "src/dead.rs".into(),
                },
                Hunk::Update {
                    path: "src/lib.rs".into(),
                    destination: Some("src/core.rs".into()),
                    chunks: vec![
                        Chunk {
                            context: Some("fn main() {".into()),
                            old: vec!["let a = 1;".into(), "let b = 2;".into()],
                            new: vec!["let a = 1;".into(), "let b = 3;".into()],
                            kept: vec![(0, 0)],
                            end_of_file: false,
                        },
                        Chunk {
                            context: None,
                            old: vec!["tail".into()],
                            new: vec!["end".into()],
                            kept: Vec::new(),
                            end_of_file: true,
                        },
                    ],
                },
            ]
        );
        assert_eq!(
            hunks[2].paths().collect::<Vec<_>>(),
            ["src/lib.rs", "src/core.rs"]
        );
    }

    #[test]
    fn reads_several_updates() {
        let hunks = parse(
            "*** Begin Patch
*** Update File: first.txt
 import foo
+bar
*** Update File: second.txt
@@
-old
+new
*** Add File: third.txt
+third
*** End Patch",
        )
        .unwrap();
        assert_eq!(hunks.len(), 3);
        assert_eq!(
            hunks[0],
            Hunk::Update {
                path: "first.txt".into(),
                destination: None,
                chunks: vec![Chunk {
                    context: None,
                    old: vec!["import foo".into()],
                    new: vec!["import foo".into(), "bar".into()],
                    kept: vec![(0, 0)],
                    end_of_file: false,
                }],
            }
        );
        assert!(matches!(&hunks[1], Hunk::Update { path, .. } if path == "second.txt"));
        assert_eq!(
            hunks[2],
            Hunk::Add {
                path: "third.txt".into(),
                contents: "third\n".into(),
            }
        );
    }

    #[test]
    fn accepts_heredocs_and_crlf() {
        let body = "*** Begin Patch\n*** Add File: a.txt\n+a\n*** End Patch";
        let expected = vec![Hunk::Add {
            path: "a.txt".into(),
            contents: "a\n".into(),
        }];
        for marker in ["<<EOF", "<<'EOF'", "<<\"EOF\""] {
            assert_eq!(
                parse(&format!("{marker}\n{body}\nEOF\n")).unwrap(),
                expected,
                "{marker}"
            );
        }
        assert_eq!(
            reason(&format!("<<\"EOF'\n{body}\nEOF\n")),
            "invalid patch: The first line of the patch must be '*** Begin Patch'"
        );
        assert_eq!(
            parse("*** Begin Patch\r\n*** Update File: f.txt\r\n@@\r\n-old\r\n+new\r\n*** End Patch\r\n")
                .unwrap(),
            [Hunk::Update {
                path: "f.txt".into(),
                destination: None,
                chunks: vec![Chunk {
                    old: vec!["old".into()],
                    new: vec!["new".into()],
                    ..Chunk::default()
                }],
            }]
        );
    }

    #[test]
    fn keeps_indented_markers_as_context() {
        let hunks = parse(
            "*** Begin Patch
*** Update File: a.txt
@@
-old a
+new a
 *** Update File: b.txt
@@
-old b
+new b
*** End Patch
",
        )
        .unwrap();
        let [Hunk::Update { path, chunks, .. }] = hunks.as_slice() else {
            panic!("one update expected: {hunks:?}")
        };
        assert_eq!(path, "a.txt");
        assert_eq!(chunks[0].old, ["old a", "*** Update File: b.txt"]);
        assert_eq!(chunks[0].kept, [(1, 1)]);
        assert_eq!(chunks[1].new, ["new b"]);
    }

    #[test]
    fn explains_malformed_patches() {
        let headers = "Valid hunk headers: '*** Add File: {path}', '*** Delete File: {path}', '*** Update File: {path}'";
        for (input, expected) in [
            (
                "bad".to_owned(),
                "invalid patch: The first line of the patch must be '*** Begin Patch'".to_owned(),
            ),
            (
                "*** Begin Patch\nbad".into(),
                "invalid patch: The last line of the patch must be '*** End Patch'".into(),
            ),
            (
                "*** Begin Patch\n*** Add File: a\n+a\n*** End Patch\nextra".into(),
                "invalid patch: The last line of the patch must be '*** End Patch'".into(),
            ),
            (
                "*** Begin Patch\nbad\n*** End Patch".into(),
                format!("invalid hunk at line 2, 'bad' is not a valid hunk header. {headers}"),
            ),
            (
                "*** Begin Patch\n*** Environment ID: remote\n*** Add File: a\n+a\n*** End Patch"
                    .into(),
                format!(
                    "invalid hunk at line 2, '*** Environment ID: remote' is not a valid hunk header. {headers}"
                ),
            ),
            (
                "*** Begin Patch\n*** Update File: test.py\n*** End Patch".into(),
                "invalid hunk at line 2, Update file hunk for path 'test.py' is empty".into(),
            ),
            (
                "*** Begin Patch\n*** Update File: a\n*** Move to: b\n*** Delete File: c\n*** End Patch"
                    .into(),
                "invalid hunk at line 2, Update file hunk for path 'a' is empty".into(),
            ),
            (
                "*** Begin Patch\n*** Update File: a\n@@\n*** End Patch".into(),
                "invalid hunk at line 4, Update hunk does not contain any lines".into(),
            ),
            (
                "*** Begin Patch\n*** Update File: a\n@@\n*** End of File\n*** End Patch".into(),
                "invalid hunk at line 4, Update hunk does not contain any lines".into(),
            ),
            (
                "*** Begin Patch\n*** Update File: a\n@@\n@@\n*** End Patch".into(),
                "invalid hunk at line 4, Unexpected line found in update hunk: '@@'. Every line should start with ' ' (context line), '+' (added line), or '-' (removed line)".into(),
            ),
            (
                "*** Begin Patch\n*** Update File: a\n@@\n-old\nbad\n*** End Patch".into(),
                "invalid hunk at line 5, Expected update hunk to start with a @@ context marker, got: 'bad'".into(),
            ),
            (
                "*** Begin Patch\n*** Update File: a\n@@\n-old\n*** End of File\n-more\n*** End Patch"
                    .into(),
                "invalid hunk at line 6, Expected update hunk to start with a @@ context marker, got: '-more'".into(),
            ),
            (
                "*** Begin Patch\n*** Add File: a\nplain\n*** End Patch".into(),
                format!("invalid hunk at line 3, 'plain' is not a valid hunk header. {headers}"),
            ),
        ] {
            assert_eq!(reason(&input), expected, "{input}");
        }
        assert!(parse("*** Begin Patch\n*** End Patch").unwrap().is_empty());
    }
}

mod matching {
    use super::*;

    #[test]
    fn applies_chunks_in_order() {
        let before = "a\nb\nc\nd\ne\n";
        assert_eq!(
            patched(before, "@@\n a\n-b\n+B\n c\n@@\n-e\n+E\n").unwrap(),
            "a\nB\nc\nd\nE\n"
        );
        assert_eq!(patched(before, "@@\n+f\n").unwrap(), "a\nb\nc\nd\ne\nf\n");
        // A trailing empty line may stand for the final newline.
        assert_eq!(patched("a\nb\n", "@@\n-b\n-\n+c\n+\n").unwrap(), "a\nc\n");
        let functions = "fn one() {\n    x\n}\nfn two() {\n    x\n}\n";
        assert_eq!(
            patched(functions, "@@ fn two() {\n-    x\n+    y\n").unwrap(),
            "fn one() {\n    x\n}\nfn two() {\n    y\n}\n"
        );
    }

    #[test]
    fn preserves_line_endings() {
        for (before, body, after) in [
            ("a\r\nb\r\n", "@@\n a\n-b\n+c\n+d\n", "a\r\nc\r\nd\r\n"),
            ("a\nb", "@@\n-a\n+x\n", "x\nb\n"),
            ("a\r\nb\nc\r\n", "@@\n a\n+x\n", "a\r\nx\r\nb\nc\r\n"),
            ("a\rb\r", "@@\n-b\n+c\n", "a\rc\r"),
            ("", "@@\n+first\n", "first\n"),
        ] {
            assert_eq!(patched(before, body).unwrap(), after, "{before:?}");
        }
    }

    #[test]
    fn anchors_end_of_file() {
        assert_eq!(
            patched("x\ny\nx\n", "@@\n-x\n+z\n*** End of File\n").unwrap(),
            "x\ny\nz\n"
        );
        let error = patched("x\ny\n", "@@\n-x\n+z\n*** End of File\n").unwrap_err();
        assert_eq!(
            error.message,
            "hunk 1: expected lines were not found at the end of the file:\nx"
        );
    }

    #[test]
    fn rejects_inexact_context() {
        let before = "    let value = 1;  \nfoo \u{2013} bar\n";
        for (body, quoted) in [
            ("@@\n-let value = 1;\n+x\n", "let value = 1;"),
            ("@@\n-    let value = 1;\n+x\n", "    let value = 1;"),
            ("@@\n-foo - bar\n+x\n", "foo - bar"),
        ] {
            let error = patched(before, body).unwrap_err();
            assert_eq!(error.code, ErrorCode::InvalidRequest, "{body}");
            assert_eq!(
                error.message,
                format!("hunk 1: expected lines were not found:\n{quoted}"),
                "{body}"
            );
        }
        assert_eq!(
            patched(before, "@@ fn missing()\n+x\n")
                .unwrap_err()
                .message,
            "hunk 1: @@ context line was not found: fn missing()"
        );
        assert_eq!(
            patched("a\nb\n", "@@\n a\n-b\n+c\n@@\n-a\n+z\n")
                .unwrap_err()
                .message,
            "hunk 2: expected lines were not found:\na"
        );
    }

    #[test]
    fn rejects_ambiguous_context() {
        let functions = "fn one() {\n    x\n}\nfn two() {\n    x\n}\n";
        assert_eq!(
            patched(functions, "@@\n-    x\n+    y\n")
                .unwrap_err()
                .message,
            "hunk 1: expected lines match 2 locations; add context lines or an @@ line to identify one:\n    x"
        );
        assert_eq!(
            patched("x\nx\nx\n", "@@\n-x\n+a\n@@\n-x\n+b\n")
                .unwrap_err()
                .message,
            "hunk 2: expected lines match 2 locations; add context lines or an @@ line to identify one:\nx"
        );
        // Each chunk only needs to be unique before the next chunk's location.
        assert_eq!(
            patched("x\n1\nx\n2\n", "@@\n-x\n+a\n@@\n-x\n+b\n").unwrap(),
            "a\n1\nb\n2\n"
        );
    }

    #[test]
    fn expresses_exact_edits() {
        for (before, body, anchor) in [
            ("a\nb", "@@\n+c\n", "b"),
            (
                "fn a() {}\r\nfn b() {}\r\n",
                "@@\n-fn b() {}\n+fn c() {}\n",
                "fn b() {}\r\n",
            ),
            ("}\n}\n}\nx\n}\n", "@@\n x\n-}\n+]\n", "x\n}\n"),
            ("a\nb\nc\n", "@@\n a\n+inserted\n b\n", "a\n"),
        ] {
            let parsed = chunks(body);
            let update = update::apply(before, &parsed).unwrap();
            let edits = update
                .edits(before, &revision(before))
                .expect("exact edits");
            assert_eq!(edits.len(), 1, "{before:?}");
            assert_eq!(edits[0].old_text, anchor, "{before:?}");
            let (after, _) = edit::apply(before, &edits, &revision(before)).unwrap();
            assert_eq!(after, update.after, "{before:?}");
        }
        let repeated = "same\n".repeat(64);
        let tail = chunks("@@\n-same\n+other\n*** End of File\n");
        let update = update::apply(&repeated, &tail).unwrap();
        let edits = update.edits(&repeated, &revision(&repeated)).unwrap();
        assert_eq!(
            edit::apply(&repeated, &edits, &revision(&repeated))
                .unwrap()
                .0,
            update.after
        );
        // An empty file and mixed line endings cannot be reproduced by exact edits.
        for (before, body) in [("", "@@\n+first\n"), ("a\nb\r\nc\n", "@@\n a\n-b\n+B\n")] {
            let update = update::apply(before, &chunks(body)).unwrap();
            assert!(
                update.edits(before, &revision(before)).is_none(),
                "{before:?}"
            );
        }
    }
}

mod resolution {
    use super::*;

    fn root() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        for (path, text) in [
            ("src/lib.rs", "fn old() {}\nfn keep() {}\n"),
            ("src/old.rs", "moved\n"),
            ("src/rename.rs", "same\n"),
            ("src/dead.rs", "dead\n"),
            ("README.md", "# Original\n"),
        ] {
            std::fs::write(root.join(path), text).unwrap();
        }
        (directory, root)
    }

    fn observed(root: &Path, paths: &[&str]) -> Vec<(String, String)> {
        paths
            .iter()
            .map(|path| {
                let text = std::fs::read_to_string(root.join(path)).unwrap();
                ((*path).to_owned(), revision(&text))
            })
            .collect()
    }

    fn targets(input: &str, observed: &[(String, String)]) -> Vec<Target> {
        parse(input)
            .unwrap()
            .into_iter()
            .map(|hunk| {
                let path = hunk.paths().next().unwrap().to_owned();
                Target {
                    hunk,
                    observed: observed
                        .iter()
                        .find(|(observed, _)| *observed == path)
                        .map(|(_, revision)| revision.clone()),
                    fault: None,
                }
            })
            .collect()
    }

    fn entries(
        root: &Path,
        input: &str,
        observed: &[(String, String)],
    ) -> Result<Vec<(FileChange, Option<Fault>)>, Fault> {
        resolve(
            Access::Restricted.scope(root),
            targets(input, observed),
            &CancellationToken::new(),
        )
    }

    fn run(root: &Path, input: &str, observed: &[(String, String)]) -> FileChanges {
        let scope = Access::Restricted.scope(root);
        let (entries, checked): (Vec<_>, Vec<_>) =
            entries(root, input, observed).unwrap().into_iter().unzip();
        match changes::plan(scope, &entries, &checked, &CancellationToken::new()) {
            Ok(planned) => changes::apply(scope, &entries, planned),
            Err(rejected) => rejected,
        }
    }

    fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_owned()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    let bytes = std::fs::read(&path).unwrap();
                    files.push((path, bytes));
                }
            }
        }
        files.sort();
        files
    }

    #[test]
    fn maps_each_directive() {
        let (_directory, root) = root();
        let observed = observed(
            &root,
            &["src/lib.rs", "src/old.rs", "src/rename.rs", "src/dead.rs"],
        );
        let before = snapshot(&root);
        let entries = entries(
            &root,
            "*** Begin Patch
*** Add File: src/nested/new.rs
+pub fn new() {}
*** Update File: src/lib.rs
@@
-fn old() {}
+fn renamed() {}
 fn keep() {}
*** Update File: src/old.rs
*** Move to: src/moved.rs
@@
-moved
+moved and edited
*** Update File: src/rename.rs
*** Move to: src/renamed.rs
@@
 same
*** Delete File: src/dead.rs
*** End Patch",
            &observed,
        )
        .unwrap();
        assert!(
            entries.iter().all(|(_, fault)| fault.is_none()),
            "{entries:?}"
        );
        let changes: Vec<_> = entries.into_iter().map(|(change, _)| change).collect();
        assert_eq!(
            changes[0],
            FileChange::Create {
                path: "src/nested/new.rs".into(),
                text: "pub fn new() {}\n".into(),
            }
        );
        let FileChange::Edit {
            path,
            expected_revision,
            edits,
        } = &changes[1]
        else {
            panic!("edit expected: {:?}", changes[1])
        };
        assert_eq!(path, "src/lib.rs");
        assert_eq!(*expected_revision, revision("fn old() {}\nfn keep() {}\n"));
        assert_eq!(edits[0].old_text, "fn old() {}\n");
        assert_eq!(edits[0].new_text, "fn renamed() {}\n");
        assert_eq!(
            changes[2..],
            [
                FileChange::Create {
                    path: "src/moved.rs".into(),
                    text: "moved and edited\n".into(),
                },
                FileChange::Delete {
                    path: "src/old.rs".into(),
                    expected_revision: revision("moved\n"),
                },
                FileChange::Move {
                    path: "src/rename.rs".into(),
                    destination: "src/renamed.rs".into(),
                    expected_revision: revision("same\n"),
                },
                FileChange::Delete {
                    path: "src/dead.rs".into(),
                    expected_revision: revision("dead\n"),
                },
            ]
        );
        assert_eq!(snapshot(&root), before);
    }

    #[test]
    fn applies_updates_in_order() {
        let (_directory, root) = root();
        let observed = observed(&root, &["src/lib.rs", "README.md"]);
        let result = run(
            &root,
            "*** Begin Patch
*** Add File: src/nested/new.rs
+pub fn new() {}
*** Update File: src/lib.rs
@@
-fn old() {}
+fn renamed() {}
*** Update File: README.md
@@
-# Original
+# Patched
*** End Patch",
            &observed,
        );
        assert_eq!(result.status, ChangesStatus::Applied, "{result:?}");
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.kind)
                .collect::<Vec<_>>(),
            [
                ChangeKind::Added,
                ChangeKind::Modified,
                ChangeKind::Modified
            ]
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn renamed() {}\nfn keep() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "# Patched\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/nested/new.rs")).unwrap(),
            "pub fn new() {}\n"
        );
        assert_eq!(
            result.files[1].revision.as_deref(),
            Some(revision("fn renamed() {}\nfn keep() {}\n").as_str())
        );
        assert!(result.files[1].excerpt.is_some());
    }

    #[test]
    fn requires_a_current_read() {
        let (_directory, root) = root();
        let patch = "*** Begin Patch
*** Add File: added.txt
+added
*** Update File: src/lib.rs
@@
-fn old() {}
+fn new() {}
*** Delete File: src/dead.rs
*** End Patch";
        let before = snapshot(&root);
        let stale = vec![("src/dead.rs".to_owned(), revision("older\n"))];
        let result = run(&root, patch, &stale);
        assert_eq!(result.status, ChangesStatus::Rejected);
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| (
                    file.status,
                    file.error.as_ref().map(|error| error.message.as_str())
                ))
                .collect::<Vec<_>>(),
            [
                (ChangeStatus::NotAttempted, None),
                (
                    ChangeStatus::Failed,
                    Some("file has not been read in this session; read the file again")
                ),
                (
                    ChangeStatus::Failed,
                    Some("file changed since this session last read it; read the file again")
                ),
            ]
        );
        assert!(
            result.files[1..]
                .iter()
                .all(|file| file.error.as_ref().unwrap().code == ErrorCode::RevisionConflict)
        );
        assert_eq!(snapshot(&root), before);
        // A read of the current revision admits the same update.
        let fresh = observed(&root, &["src/lib.rs"]);
        let update = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-fn old() {}\n+fn new() {}\n*** End Patch";
        assert_eq!(run(&root, update, &fresh).status, ChangesStatus::Applied);
        assert_eq!(
            run(&root, update, &fresh).files[0]
                .error
                .as_ref()
                .unwrap()
                .message,
            "file changed since this session last read it; read the file again"
        );
    }

    #[test]
    fn rejects_whole_patch_without_writing() {
        let (_directory, root) = root();
        let observed = observed(&root, &["src/lib.rs", "README.md"]);
        let before = snapshot(&root);
        let result = run(
            &root,
            "*** Begin Patch
*** Add File: added.txt
+added
*** Update File: README.md
@@
-# Original
+# Patched
*** Update File: src/lib.rs
@@
-fn old() {}
+fn new() {}
@@
-fn absent() {}
+fn other() {}
*** End Patch",
            &observed,
        );
        assert_eq!(result.status, ChangesStatus::Rejected);
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.status)
                .collect::<Vec<_>>(),
            [
                ChangeStatus::NotAttempted,
                ChangeStatus::NotAttempted,
                ChangeStatus::Failed
            ]
        );
        assert_eq!(result.files[2].path, "src/lib.rs");
        assert_eq!(
            result.files[2].error.as_ref().unwrap().message,
            "hunk 2: expected lines were not found:\nfn absent() {}"
        );
        assert_eq!(snapshot(&root), before);
    }

    #[test]
    fn rechecks_preimage_before_writing() {
        let (_directory, root) = root();
        let observed = observed(&root, &["src/lib.rs", "README.md"]);
        let scope = Access::Restricted.scope(&root);
        let (entries, checked): (Vec<_>, Vec<_>) = entries(
            &root,
            "*** Begin Patch
*** Add File: first.txt
+first
*** Update File: src/lib.rs
@@
-fn old() {}
+fn new() {}
*** Update File: README.md
@@
-# Original
+# Never
*** End Patch",
            &observed,
        )
        .unwrap()
        .into_iter()
        .unzip();
        let planned = changes::plan(scope, &entries, &checked, &CancellationToken::new()).unwrap();
        std::fs::write(root.join("src/lib.rs"), "fn external() {}\n").unwrap();
        let result = changes::apply(scope, &entries, planned);
        assert_eq!(result.status, ChangesStatus::Partial);
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.status)
                .collect::<Vec<_>>(),
            [
                ChangeStatus::Applied,
                ChangeStatus::Failed,
                ChangeStatus::NotAttempted
            ]
        );
        assert_eq!(
            result.files[1].error.as_ref().map(|error| error.code),
            Some(ErrorCode::RevisionConflict)
        );
        assert_eq!(
            std::fs::read_to_string(root.join("src/lib.rs")).unwrap(),
            "fn external() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("README.md")).unwrap(),
            "# Original\n"
        );
    }

    #[test]
    fn writes_whole_files_without_exact_edits() {
        let (_directory, root) = root();
        std::fs::write(root.join("mixed.txt"), "a\nb\r\nc\n").unwrap();
        std::fs::write(root.join("empty.txt"), "").unwrap();
        let observed = observed(&root, &["mixed.txt", "empty.txt"]);
        let entries = entries(
            &root,
            "*** Begin Patch
*** Update File: mixed.txt
@@
 a
-b
+B
*** Update File: empty.txt
@@
+first
*** End Patch",
            &observed,
        )
        .unwrap();
        assert_eq!(
            entries,
            [
                (
                    FileChange::Write {
                        path: "mixed.txt".into(),
                        text: "a\nB\nc\n".into(),
                        expected_revision: revision("a\nb\r\nc\n"),
                    },
                    None
                ),
                (
                    FileChange::Write {
                        path: "empty.txt".into(),
                        text: "first\n".into(),
                        expected_revision: revision(""),
                    },
                    None
                ),
            ]
        );
    }

    #[test]
    fn keeps_resource_faults_per_hunk() {
        let (_directory, root) = root();
        let denied = Fault::new(ErrorCode::Conflict, "entry contains Node storage");
        let mut targets = targets(
            "*** Begin Patch
*** Add File: allowed.txt
+allowed
*** Update File: src/old.rs
*** Move to: src/moved.rs
@@
-moved
+edited
*** End Patch",
            &[],
        );
        targets[1].fault = Some(denied.clone());
        let entries = resolve(
            Access::Restricted.scope(&root),
            targets,
            &CancellationToken::new(),
        )
        .unwrap();
        assert_eq!(entries[0].1, None);
        assert_eq!(
            entries[1],
            (
                FileChange::Move {
                    path: "src/old.rs".into(),
                    destination: "src/moved.rs".into(),
                    expected_revision: String::new(),
                },
                Some(denied)
            )
        );
    }

    #[test]
    fn bounds_file_count() {
        let (_directory, root) = root();
        let adds: String = (0..=MAX_CHANGES)
            .map(|index| format!("*** Add File: {index}.txt\n+{index}\n"))
            .collect();
        let error =
            entries(&root, &format!("*** Begin Patch\n{adds}*** End Patch"), &[]).unwrap_err();
        assert_eq!(
            error.message,
            "a patch can change at most 16 files; a moved and edited file counts twice"
        );
        let adds: String = (1..MAX_CHANGES)
            .map(|index| format!("*** Add File: {index}.txt\n+{index}\n"))
            .collect();
        let observed = observed(&root, &["src/old.rs"]);
        let moved = "*** Update File: src/old.rs\n*** Move to: src/moved.rs\n@@\n-moved\n+edited\n";
        assert!(
            entries(
                &root,
                &format!("*** Begin Patch\n{adds}{moved}*** End Patch"),
                &observed
            )
            .is_err()
        );
        assert_eq!(
            entries(&root, "*** Begin Patch\n*** End Patch", &[])
                .unwrap_err()
                .message,
            "patch contains no file changes"
        );
    }
}
