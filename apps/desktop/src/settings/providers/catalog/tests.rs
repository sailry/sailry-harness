use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn shows_status(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |owner, _| {
            owner.section = crate::settings::Section::Providers;
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let owner = owner.unwrap();
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("catalog-empty").is_some());
    assert!(visual.debug_bounds("catalog-updated").is_none());
    assert!(
        visual
            .debug_bounds("settings-row-provider_catalog_last_update")
            .is_some()
    );
    visual.update(|_, cx| owner.update(cx, |owner, cx| owner.refresh_catalog(cx)));
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("catalog-empty").is_none());
    assert!(visual.debug_bounds("catalog-updated").is_some());
    assert!(
        visual
            .debug_bounds("settings-row-provider_catalog_last_update")
            .is_some()
    );
}
