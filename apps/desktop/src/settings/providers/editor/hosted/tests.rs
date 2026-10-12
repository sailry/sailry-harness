use super::*;
use core::prelude::v1::test;
use sailry_protocol::conversation::{ModelApi, connection::Options};

#[gpui::test]
fn selects_routes_and_seed_models(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let visual = cx.add_empty_window();
    visual.update(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        for vendor in [
            Vendor::Qwen,
            Vendor::Moonshot,
            Vendor::MiniMax,
            Vendor::Zhipu,
            Vendor::Doubao,
        ] {
            let editor = cx.new(|cx| Editor::new(owner.clone(), None, window, cx));
            editor.update(cx, |editor, cx| {
                editor.select_preset(Preset::Hosted(vendor), window, cx);
                editor.name.update(cx, |input, cx| {
                    input.set_value("Hosted fixture", window, cx)
                });
                editor.select_product(Product::Coding, window, cx);
                for &(region, endpoint) in vendor.coding_endpoints() {
                    editor.select_region(region, window, cx);
                    assert_eq!(
                        editor.connection(cx).unwrap(),
                        (ModelApi::ChatCompletions, endpoint.into())
                    );
                    assert_eq!(editor.region.read(cx).selected_value(), Some(&region));
                    assert_eq!(
                        editor.offering.read(cx).selected_value(),
                        Some(&Product::Coding)
                    );
                    assert_eq!(editor.models.len(), 1);
                    assert_eq!(
                        editor.models[0].id.read(cx).value().as_str(),
                        vendor.coding_model().unwrap()
                    );
                    assert_eq!(editor.metadata_only(), vendor != Vendor::MiniMax);
                    editor
                        .credential
                        .update(cx, |input, cx| input.set_value("synthetic-key", window, cx));
                    let channel = editor.channel(cx).unwrap();
                    assert_eq!(
                        channel.options,
                        Some(Options::Hosted {
                            product: Product::Coding
                        })
                    );
                    assert_eq!(channel.endpoint, endpoint);
                    assert!(channel.label().contains(tr(vendor.coding_key()).as_ref()));
                    assert!(channel.label().contains(tr(region.key()).as_ref()));
                }
            });
        }
    });
}

#[gpui::test]
fn preserves_unset_metadata_and_original_endpoint(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let visual = cx.add_empty_window();
    visual.update(|window, cx| {
        for endpoint in [
            "https://api.minimax.chat/v1",
            "https://api.minimaxi.com/v1",
            "https://api.minimax.io/v1/",
        ] {
            let owner = cx.new(|cx| Workspace::new(window, cx));
            owner.update(cx, |owner, _| {
                let channel = &mut owner.providers.channels[0];
                channel.preset = Preset::Hosted(Vendor::MiniMax);
                channel.endpoint = endpoint.into();
                channel.options = None;
            });
            let editor = cx.new(|cx| Editor::new(owner, Some(0), window, cx));
            editor.update(cx, |editor, cx| {
                editor.name.update(cx, |input, cx| {
                    input.set_value("Renamed fixture", window, cx)
                });
                assert_eq!(editor.connection(cx).unwrap().1, endpoint);
                let channel = editor.channel(cx).unwrap();
                assert_eq!(channel.options, None);
                assert_eq!(channel.endpoint, endpoint);
                editor.select_product(Product::Metered, window, cx);
                editor
                    .credential
                    .update(cx, |input, cx| input.set_value("synthetic-key", window, cx));
                editor.add_model(window, cx);
                editor.models[0]
                    .id
                    .update(cx, |input, cx| input.set_value("manual-model", window, cx));
                assert_eq!(
                    editor.channel(cx).unwrap().options,
                    Some(Options::Hosted {
                        product: Product::Metered
                    })
                );
            });
        }
    });
}

#[gpui::test]
fn pending_choices_leave_drafts_unchanged(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let visual = cx.add_empty_window();
    visual.update(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        let editor = cx.new(|cx| Editor::new(owner, None, window, cx));
        editor.update(cx, |editor, cx| {
            editor.select_preset(Preset::Hosted(Vendor::Qwen), window, cx);
            editor
                .credential
                .update(cx, |input, cx| input.set_value("synthetic-key", window, cx));
            editor.pending = true;
            editor.select_product(Product::Coding, window, cx);
            editor.select_region(Region::Global, window, cx);
            assert_eq!(editor.product(), Product::Metered);
            assert_eq!(editor.connection(cx).unwrap().1, Vendor::Qwen.endpoint());
            assert_eq!(editor.credential.read(cx).value(), "synthetic-key");
        });
    });
}

#[gpui::test]
fn unknown_saved_product_stays_unselected(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let visual = cx.add_empty_window();
    visual.update(|window, cx| {
        for vendor in [
            Vendor::Qwen,
            Vendor::Moonshot,
            Vendor::MiniMax,
            Vendor::Zhipu,
        ] {
            let endpoint = vendor.coding_endpoints()[1].1;
            let owner = cx.new(|cx| Workspace::new(window, cx));
            owner.update(cx, |owner, _| {
                let channel = &mut owner.providers.channels[0];
                channel.preset = Preset::Hosted(vendor);
                channel.endpoint = endpoint.into();
                channel.options = None;
            });
            let editor = cx.new(|cx| Editor::new(owner, Some(0), window, cx));
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.offering.read(cx).selected_value(), None);
                assert_eq!(editor.offering.read(cx).selected_index(cx), None);
                assert_eq!(
                    editor.region.read(cx).selected_value(),
                    Some(&Region::Global)
                );
                assert_eq!(editor.connection(cx).unwrap().1, endpoint);
                assert_eq!(editor.channel(cx).unwrap().options, None);
                let original = editor.channel(cx).unwrap();
                editor.select_region(Region::China, window, cx);
                assert!(editor.channel(cx).unwrap() == original);
                assert_eq!(
                    editor.region.read(cx).selected_value(),
                    Some(&Region::Global)
                );
                editor.select_product(Product::Coding, window, cx);
                assert_eq!(
                    editor.offering.read(cx).selected_value(),
                    Some(&Product::Coding)
                );
                editor.select_region(Region::China, window, cx);
                assert_eq!(
                    editor.connection(cx).unwrap().1,
                    vendor.coding_endpoints()[0].1
                );
            });
        }
    });
}
