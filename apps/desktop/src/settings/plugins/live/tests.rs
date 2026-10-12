use super::*;
use crate::settings::plugins::test_support::{Fixture, init, tap, wait};
use core::prelude::v1::test;
use std::sync::atomic::Ordering;

#[gpui::test]
fn recovery_preserves_the_original_host_and_request(cx: &mut TestAppContext) {
    init(cx);
    for (remote, bundled) in [false, true]
        .into_iter()
        .flat_map(|remote| [false, true].map(move |bundled| (remote, bundled)))
    {
        let fixture = Fixture::new(remote);
        let (owner, visual) = fixture.mount(cx);
        let package = owner.read_with(visual, |owner, _| owner.plugin_catalog.packages[0].clone());
        fixture.transport.mode.store(1, Ordering::SeqCst);
        visual.update(|window, cx| {
            owner.update(cx, |owner, cx| {
                if bundled {
                    owner.install_bundled(&package.name, window, cx);
                } else {
                    owner.toggle_plugin(&package, true, window, cx);
                }
            });
        });
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .action
                .as_ref()
                .is_some_and(|action| !action.pending)
        });
        let captured = owner.read_with(visual, |owner, _| {
            owner
                .plugin_catalog
                .action
                .as_ref()
                .unwrap()
                .request
                .clone()
        });
        fixture.bind(&owner, visual, true);
        let other = owner.read_with(visual, |owner, _| owner.plugin_catalog.packages.clone());
        assert!(owner.read_with(visual, |owner, _| owner.plugin_catalog.action.is_none()));
        tap(
            visual,
            Box::leak(format!("installation-retry-{}", captured.id).into_boxed_str()),
        );
        wait(visual, |cx| !crate::installations::visible(cx));
        assert_eq!(
            *fixture.transport.requests.lock().unwrap(),
            [captured.id, captured.id]
        );
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.plugin_catalog.packages, other);
            assert!(owner.plugin_catalog.action.is_none());
            assert_eq!(
                owner
                    .provider_link
                    .as_ref()
                    .unwrap()
                    .binding
                    .client
                    .target(),
                fixture.other.id()
            );
        });
        fixture.close(visual);
    }
}
