use super::*;
use crate::settings::providers::{
    editor::open,
    vendors::{Region, Vendor},
};
use core::prelude::v1::test;
use sailry_protocol::{
    Secret,
    conversation::connection::{Options, Product},
};

fn mount<'a>(
    fixture: &crate::settings::providers::fixture::Fixture,
    transport: Arc<dyn Transport>,
    cx: &'a mut TestAppContext,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.bind_providers(transport, fixture.runtime.clone(), "Hosted Node".into(), cx);
        });
        owner = Some(workspace.clone());
        Root::new(cx.new(|_| Surface(workspace)), window, cx)
    });
    let owner = owner.unwrap();
    wait(visual, |cx| {
        owner.read(cx).provider_link.as_ref().unwrap().connected
    });
    (owner, visual)
}

#[gpui::test]
fn saves_and_reopens_regions_and_products(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let fixture = crate::settings::providers::fixture::Fixture::empty();
    for transport in &fixture.transports {
        let client = Client::new(transport.clone());
        let (owner, visual) = mount(&fixture, transport.clone(), cx);
        for vendor in [
            Vendor::Qwen,
            Vendor::Moonshot,
            Vendor::MiniMax,
            Vendor::Zhipu,
            Vendor::Doubao,
        ] {
            for product in [Product::Metered, Product::Coding] {
                let routes: Vec<_> = match product {
                    Product::Metered if vendor == Vendor::Doubao => {
                        vec![(Region::China, vendor.endpoint())]
                    }
                    Product::Metered => vendor.endpoints().to_vec(),
                    Product::Coding => vendor.coding_endpoints().to_vec(),
                };
                for (region, endpoint) in routes {
                    let editor = visual.update(|window, cx| open(owner.clone(), None, window, cx));
                    let provider_id = editor.read_with(visual, |editor, _| editor.provider_id);
                    visual.update(|window, cx| {
                        editor.update(cx, |editor, cx| {
                            editor.select_preset(Preset::Hosted(vendor), window, cx);
                            editor.select_product(product, window, cx);
                            editor.select_region(region, window, cx);
                            editor.name.update(cx, |input, cx| {
                                input.set_value(
                                    format!("{vendor:?} {product:?} {region:?}"),
                                    window,
                                    cx,
                                )
                            });
                            editor.credential.update(cx, |input, cx| {
                                input.set_value("synthetic-hosted-key", window, cx)
                            });
                            if editor.models.is_empty() {
                                editor.add_model(window, cx);
                                editor.models[0].id.update(cx, |input, cx| {
                                    input.set_value("manual-fixture", window, cx)
                                });
                            }
                            editor.save(window, cx);
                        })
                    });
                    wait(visual, |cx| {
                        !editor.read(cx).pending
                            && owner
                                .read(cx)
                                .provider_link
                                .as_ref()
                                .unwrap()
                                .providers
                                .values()
                                .any(|provider| provider.id == provider_id)
                    });
                    let (id, saved) = owner.read_with(visual, |owner, _| {
                        owner
                            .provider_link
                            .as_ref()
                            .unwrap()
                            .providers
                            .iter()
                            .find(|(_, provider)| provider.id == provider_id)
                            .map(|(id, provider)| (*id, provider.clone()))
                            .unwrap()
                    });
                    assert_eq!(saved.endpoint, endpoint);
                    assert_eq!(saved.api, ModelApi::ChatCompletions);
                    assert_eq!(saved.options, Some(Options::Hosted { product }));
                    assert!(saved.credential.is_some());
                    let reopened =
                        visual.update(|window, cx| open(owner.clone(), Some(id), window, cx));
                    wait(visual, |cx| !reopened.read(cx).pending);
                    reopened.read_with(visual, |editor, cx| {
                        assert_eq!(editor.preset, Preset::Hosted(vendor));
                        assert_eq!(editor.product(), product);
                        assert_eq!(editor.connection(cx).unwrap().1, endpoint);
                        assert_eq!(editor.offering.read(cx).selected_value(), Some(&product));
                        if vendor != Vendor::Doubao || product == Product::Coding {
                            assert_eq!(editor.region.read(cx).selected_value(), Some(&region));
                        }
                        assert_eq!(editor.channel(cx).unwrap().options, saved.options);
                        assert_eq!(editor.credential.read(cx).value(), "synthetic-hosted-key");
                    });
                    visual.update(|window, cx| {
                        reopened.update(cx, |editor, cx| editor.close(window, cx));
                        window.close_dialog(cx);
                    });
                    let Output::ProviderKey(key) = fixture
                        .runtime
                        .block_on(client.execute(client.prepare(Command::ReadProviderKey {
                            provider: saved.id,
                            expected_revision: saved.revision,
                        })))
                        .unwrap()
                    else {
                        panic!("provider key expected")
                    };
                    assert_eq!(
                        key.as_ref().map(Secret::expose),
                        Some("synthetic-hosted-key")
                    );
                }
            }
        }
        visual.update(|window, _| window.remove_window());
    }
    fixture.close();
}

#[gpui::test]
fn unchanged_addresses_keep_credentials(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    let fixture = crate::settings::providers::fixture::Fixture::empty();
    for transport in &fixture.transports {
        let client = Client::new(transport.clone());
        let (owner, visual) = mount(&fixture, transport.clone(), cx);
        for (vendor, endpoint) in [
            (Vendor::MiniMax, "https://api.minimax.chat/v1"),
            (Vendor::MiniMax, "https://api.minimaxi.com/v1"),
            (Vendor::Qwen, Vendor::Qwen.coding_endpoints()[1].1),
            (Vendor::Moonshot, Vendor::Moonshot.coding_endpoints()[1].1),
            (Vendor::MiniMax, Vendor::MiniMax.coding_endpoints()[1].1),
            (Vendor::Zhipu, Vendor::Zhipu.coding_endpoints()[1].1),
        ] {
            let mut provider = crate::provider_fixture::provider(
                sailry_protocol::Authentication::ApiKey,
                ModelApi::ChatCompletions,
            );
            provider.endpoint = endpoint.into();
            let sample = super::super::super::super::data::Model::example("manual-fixture");
            provider.models = vec![crate::settings::providers::live::model(&sample)];
            provider.default_model = "manual-fixture".into();
            let Output::Provider(saved) = fixture
                .runtime
                .block_on(client.execute(client.prepare(Command::SaveProvider {
                    expected_revision: 0,
                    provider,
                    secret: Some(Secret::new("synthetic-legacy-key".into())),
                })))
                .unwrap()
            else {
                panic!("provider expected")
            };
            wait(visual, |cx| {
                owner
                    .read(cx)
                    .provider_link
                    .as_ref()
                    .unwrap()
                    .providers
                    .values()
                    .any(|provider| provider.id == saved.id)
            });
            let id = owner.read_with(visual, |owner, _| {
                *owner
                    .provider_link
                    .as_ref()
                    .unwrap()
                    .providers
                    .iter()
                    .find(|(_, provider)| provider.id == saved.id)
                    .unwrap()
                    .0
            });
            let editor = visual.update(|window, cx| open(owner.clone(), Some(id), window, cx));
            wait(visual, |cx| !editor.read(cx).pending);
            visual.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    assert_eq!(editor.preset, Preset::Hosted(vendor));
                    assert_eq!(editor.offering.read(cx).selected_value(), None);
                    assert_eq!(editor.connection(cx).unwrap().1, endpoint);
                    assert_eq!(editor.channel(cx).unwrap().options, None);
                    editor.name.update(cx, |input, cx| {
                        input.set_value("Renamed legacy", window, cx)
                    });
                    editor.save(window, cx);
                })
            });
            wait(visual, |cx| {
                !editor.read(cx).pending
                    && owner.read(cx).provider_link.as_ref().unwrap().providers[&id].revision
                        == saved.revision + 1
            });
            let next = owner.read_with(visual, |owner, _| {
                owner.provider_link.as_ref().unwrap().providers[&id].clone()
            });
            assert_eq!(next.endpoint, endpoint);
            assert_eq!(next.options, None);
            assert_eq!(next.credential, saved.credential);
        }
        visual.update(|window, _| window.remove_window());
    }
    fixture.close();
}
