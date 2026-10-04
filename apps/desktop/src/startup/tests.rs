use super::*;

#[test]
fn uses_shared_default() {
    let Some(Launch::Node(options)) = parse(std::iter::empty()).unwrap() else {
        panic!("Node launch expected")
    };
    assert_eq!(
        options.data_dir,
        sailry_node_runtime::default_data_dir().unwrap()
    );
    assert!(options.relays.is_empty());
}

#[test]
fn preserves_explicit_path() {
    let path = std::env::temp_dir().join("指定数据 🙂");
    let Some(Launch::Node(options)) = parse(
        [
            "--relay".into(),
            "https://relay.example".into(),
            "--data-dir".into(),
            path.clone().into_os_string(),
            "--relay".into(),
            "https://other.example".into(),
        ]
        .into_iter(),
    )
    .unwrap() else {
        panic!("Node launch expected")
    };
    assert_eq!(options.data_dir, path);
    assert_eq!(
        options.relays,
        ["https://relay.example", "https://other.example"]
    );
}

#[test]
fn selects_preview() {
    assert!(matches!(
        parse(["--preview".into()].into_iter()).unwrap(),
        Some(Launch::Preview)
    ));
}

#[test]
fn selects_help() {
    for flag in ["--help", "-h"] {
        assert!(parse([flag.into()].into_iter()).unwrap().is_none());
    }
}

#[test]
fn rejects_ambiguous_options() {
    let path = std::env::temp_dir().into_os_string();
    for args in [
        vec!["--data-dir".into()],
        vec!["--data-dir".into(), "relative".into()],
        vec![
            "--data-dir".into(),
            path.clone(),
            "--data-dir".into(),
            path.clone(),
        ],
        vec!["--preview".into(), "--data-dir".into(), path.clone()],
        vec!["--data-dir".into(), path, "--preview".into()],
        vec![
            "--preview".into(),
            "--relay".into(),
            "https://relay.example".into(),
        ],
        vec!["--preview".into(), "--preview".into()],
        vec!["--relay".into()],
        vec!["--help".into(), "--preview".into()],
        vec!["--unknown".into()],
    ] {
        assert!(parse(args.into_iter()).is_err());
    }
}

#[cfg(unix)]
#[test]
fn preserves_os_paths() {
    use std::os::unix::ffi::OsStringExt;
    let path = std::env::temp_dir().join(OsString::from_vec(vec![b'd', 0xff]));
    let Some(Launch::Node(options)) =
        parse(["--data-dir".into(), path.clone().into_os_string()].into_iter()).unwrap()
    else {
        panic!("Node launch expected")
    };
    assert_eq!(options.data_dir, path);
}
