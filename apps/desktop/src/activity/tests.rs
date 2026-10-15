use core::prelude::v1::test;

#[test]
fn status_icons_use_embedded_catalog() {
    use gpui_kit::AssetSource;
    use sailry_client::activity::Lane;
    for lane in [
        Lane::Waiting,
        Lane::Running,
        Lane::Completed,
        Lane::Failed,
        Lane::Idle,
    ] {
        assert!(
            crate::assets::Assets
                .load(super::icons::path(lane))
                .unwrap()
                .is_some()
        );
    }
    for path in [
        "icons/check-check.svg",
        "icons/circle-check.svg",
        "icons/shield-alert.svg",
        "icons/message-circle-question-mark.svg",
    ] {
        assert!(crate::assets::Assets.load(path).unwrap().is_some());
    }
}
