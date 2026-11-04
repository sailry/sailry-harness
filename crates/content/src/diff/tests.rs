use core::prelude::v1::test;

#[test]
fn preserves_document_behavior() {
    for (scenario, check) in [
        (
            "parses_captured_hunks_and_new_files",
            (|scenario: &'static str| {
                let path = "nested/一 two.txt";
                let files =
                    crate::diff::files("@@ -30,2 +40,2 @@\n same\n-old\n+new\n", path, false)
                        .unwrap();
                assert_eq!(files.len(), 1, "{scenario}");
                assert_eq!(files[0].path().as_ref(), path, "{scenario}");
                assert_eq!(
                    (files[0].additions(), files[0].deletions()),
                    (1, 1),
                    "{scenario}"
                );
                for source in ["", "一\ntwo 🙂\n", "one\ntwo", "--- content\n+++ content\n"] {
                    let files = crate::diff::files(source, path, true).unwrap();
                    assert_eq!(files.len(), 1, "{scenario}");
                    assert_eq!(files[0].path().as_ref(), path, "{scenario}");
                    assert_eq!(files[0].deletions(), 0, "{scenario}");
                    assert_eq!(files[0].additions(), source.lines().count(), "{scenario}");
                }
            }) as fn(&'static str),
        ),
        (
            "preserves_binary_and_extended_headers",
            (|scenario: &'static str| {
                let files = crate::diff::files("diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1234567\nBinary files /dev/null and b/image.png differ\n", "ignored", false).unwrap();
                assert_eq!(files[0].path().as_ref(), "image.png", "{scenario}");
                assert!(files[0].is_binary(), "{scenario}");
                assert!(
                    files[0]
                        .extended_headers()
                        .iter()
                        .any(|line| line.as_ref() == "new file mode 100644"),
                    "{scenario}"
                );
                let text =
                    "@@ -0,0 +1,2 @@\n+Binary files a/file and b/file differ\n+GIT binary patch\n";
                assert!(
                    !crate::diff::files(text, "text.txt", false).unwrap()[0].is_binary(),
                    "{scenario}"
                );
            }),
        ),
        (
            "retains_available_truncated_rows",
            (|scenario: &'static str| {
                let files = crate::diff::files(
                    "@@ -7,20 +8,20 @@\n-old\n+new\n same\n",
                    "partial.txt",
                    false,
                )
                .unwrap();
                assert_eq!(
                    (files[0].additions(), files[0].deletions()),
                    (1, 1),
                    "{scenario}"
                );
                assert!(
                    crate::diff::files("@@ invalid @@\n+one\n", "bad.txt", false).is_err(),
                    "{scenario}"
                );
                assert!(
                    crate::diff::files("@@ -1 +1 @@\n-one\n-two\n+new\n", "bad.txt", false)
                        .is_err(),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
