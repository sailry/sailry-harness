use super::*;

#[test]
fn launch_options() {
    for (scenario, check) in [
        (
            "uses_shared_default",
            (|_scenario: &str| {
                let Some(Launch::Node(options)) = parse(std::iter::empty()).unwrap() else {
                    panic!("Node launch expected")
                };
                assert_eq!(
                    options.data_dir,
                    sailry_node_runtime::default_data_dir().unwrap(),
                    "uses shared default"
                );
                assert!(options.relays.is_empty(), "uses shared default");
            }) as fn(&str),
        ),
        (
            "preserves_explicit_path",
            (|_scenario: &str| {
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
                assert_eq!(options.data_dir, path, "preserves explicit path");
                assert_eq!(
                    options.relays,
                    ["https://relay.example", "https://other.example"],
                    "preserves explicit path"
                );
            }),
        ),
        (
            "selects_preview",
            (|_scenario: &str| {
                assert!(
                    matches!(
                        parse(["--preview".into()].into_iter()).unwrap(),
                        Some(Launch::Preview)
                    ),
                    "selects preview"
                );
            }),
        ),
        (
            "selects_help",
            (|_scenario: &str| {
                for flag in ["--help", "-h"] {
                    assert!(
                        parse([flag.into()].into_iter()).unwrap().is_none(),
                        "selects help"
                    );
                }
            }),
        ),
        (
            "selects_theme_check",
            (|_scenario: &str| {
                let path = PathBuf::from("themes/fixture");
                let Some(Launch::CheckTheme(selected)) =
                    parse(["--check-theme".into(), path.clone().into_os_string()].into_iter())
                        .unwrap()
                else {
                    panic!("theme check expected")
                };
                assert_eq!(selected, path, "selects theme check");
            }),
        ),
        (
            "rejects_mixed_theme_options",
            (|_scenario: &str| {
                for args in [
                    vec!["--check-theme"],
                    vec!["--check-theme", "fixture", "--check-theme", "other"],
                    vec!["--check-theme", "fixture", "--preview"],
                    vec!["--preview", "--check-theme", "fixture"],
                    vec![
                        "--check-theme",
                        "fixture",
                        "--relay",
                        "https://relay.example",
                    ],
                ] {
                    assert!(
                        parse(args.into_iter().map(OsString::from)).is_err(),
                        "rejects mixed theme options"
                    );
                }
                let path = std::env::temp_dir().into_os_string();
                assert!(
                    parse(
                        [
                            "--data-dir".into(),
                            path,
                            "--check-theme".into(),
                            "fixture".into()
                        ]
                        .into_iter()
                    )
                    .is_err(),
                    "rejects mixed theme options"
                );
            }),
        ),
        (
            "rejects_ambiguous_options",
            (|_scenario: &str| {
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
                    assert!(
                        parse(args.into_iter()).is_err(),
                        "rejects ambiguous options"
                    );
                }
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
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
