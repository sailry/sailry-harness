use super::*;
use core::prelude::v1::test;

#[test]
fn rejects_sequences_and_unmodified_keys() {
    assert!(parse("").unwrap().is_none());
    assert!(parse("secondary-alt-space").unwrap().is_some());
    assert!(parse("secondary-alt-s").unwrap().is_some());
    assert!(parse("s").is_err());
    assert!(parse("ctrl-k ctrl-s").is_err());
}

#[gpui::test]
fn unavailable_native_registration_preserves_preferences(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
    let node = runtime
        .block_on(sailry_node_runtime::Node::start_isolated(
            directory.path().join("node"),
        ))
        .unwrap();
    cx.update(|cx| {
        cx.set_global(crate::backend::Services {
            profile: node.profile().to_path_buf(),
            runtime: runtime.clone(),
            link: node.link().clone(),
            local: node.local(),
            relay_enabled: false,
        });
        crate::shell::shortcuts::init(cx);
        crate::shortcuts::init(cx);
        let previous = crate::shortcuts::key("system.activate", cx);
        assert_eq!(
            crate::shortcuts::save("system.activate", Some("secondary-alt-z"), cx),
            Err("shortcut_unavailable")
        );
        assert_eq!(crate::shortcuts::key("system.activate", cx), previous);
        assert!(
            !crate::preferences::data(cx)
                .shortcuts
                .unwrap_or_default()
                .contains_key("system.activate")
        );
    });
    runtime.block_on(node.shutdown()).unwrap();
}
