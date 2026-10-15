use core::prelude::v1::test;

#[test]
fn parses_captured_hunks_and_new_files() {
    let path = "nested/一 two.txt";
    let files = crate::diff::files("@@ -30,2 +40,2 @@\n same\n-old\n+new\n", path, false).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path().as_ref(), path);
    assert_eq!((files[0].additions(), files[0].deletions()), (1, 1));
    for source in ["", "一\ntwo 🙂\n", "one\ntwo", "--- content\n+++ content\n"] {
        let files = crate::diff::files(source, path, true).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path().as_ref(), path);
        assert_eq!(files[0].deletions(), 0);
        assert_eq!(files[0].additions(), source.lines().count());
    }
}

#[test]
fn preserves_binary_and_extended_headers() {
    let files = crate::diff::files("diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..1234567\nBinary files /dev/null and b/image.png differ\n", "ignored", false).unwrap();
    assert_eq!(files[0].path().as_ref(), "image.png");
    assert!(files[0].is_binary());
    assert!(
        files[0]
            .extended_headers()
            .iter()
            .any(|line| line.as_ref() == "new file mode 100644")
    );
    let text = "@@ -0,0 +1,2 @@\n+Binary files a/file and b/file differ\n+GIT binary patch\n";
    assert!(!crate::diff::files(text, "text.txt", false).unwrap()[0].is_binary());
}

#[test]
fn retains_available_truncated_rows() {
    let files = crate::diff::files(
        "@@ -7,20 +8,20 @@\n-old\n+new\n same\n",
        "partial.txt",
        false,
    )
    .unwrap();
    assert_eq!((files[0].additions(), files[0].deletions()), (1, 1));
    assert!(crate::diff::files("@@ invalid @@\n+one\n", "bad.txt", false).is_err());
    assert!(crate::diff::files("@@ -1 +1 @@\n-one\n-two\n+new\n", "bad.txt", false).is_err());
}
