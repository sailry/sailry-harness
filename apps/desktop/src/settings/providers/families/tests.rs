use super::*;
use crate::settings::Section;
use core::prelude::v1::test;

#[gpui::test]
fn fixed_categories(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.providers.channels.clear();
            workspace.select(Section::Providers, cx);
        });
        owner = Some(workspace.clone());
        Root::new(workspace, window, cx)
    });
    let owner = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(2600.), px(820.)));
    draw(visual);
    let tabs = visual.debug_bounds("provider-families").unwrap();
    let available = visual
        .debug_bounds("settings-title-provider_channels")
        .unwrap();
    assert!(tabs.size.width < available.size.width);
    for category in ModelCategory::ALL {
        visual.update(|_, cx| {
            owner.update(cx, |owner, cx| {
                owner.providers.category = category;
                cx.notify();
            })
        });
        draw(visual);
        draw(visual);
        let tabs = visual.debug_bounds("provider-families").unwrap();
        let tab = visual.debug_bounds(category.key()).unwrap();
        assert!(tab.left() >= tabs.left() && tab.right() <= tabs.right());
        visual.simulate_click(tab.center(), Modifiers::default());
        draw(visual);
        owner.read_with(visual, |owner, _| {
            assert_eq!(owner.providers.category, category)
        });
    }
    visual.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            let sample = Store::default().channels.remove(0);
            for preset in [
                Preset::OpenAi,
                Preset::ChatGpt,
                Preset::Anthropic,
                Preset::Bedrock,
            ] {
                owner.providers.save(
                    None,
                    super::super::Channel {
                        preset,
                        ..sample.clone()
                    },
                );
            }
            cx.notify();
        });
    });
    draw(visual);
    assert!(visual.debug_bounds("provider-4").is_some());
    assert!(visual.debug_bounds("provider-1").is_none());
    visual.update(|_, cx| {
        owner.update(cx, |owner, cx| {
            owner.providers.remove(4);
            assert_eq!(owner.providers.category, ModelCategory::Other);
            cx.notify();
        });
    });
    draw(visual);
    assert!(visual.debug_bounds("provider-4").is_none());
    assert!(visual.debug_bounds("provider_other").is_some());
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[test]
fn brand_channels() {
    for (preset, category) in [
        (Preset::CompatibleResponses, ModelCategory::OpenAi),
        (Preset::CompatibleChat, ModelCategory::OpenAi),
        (Preset::CompatibleAnthropic, ModelCategory::Anthropic),
        (Preset::CompatibleGemini, ModelCategory::Gemini),
        (Preset::OpenCodeGo, ModelCategory::OpenCodeGo),
        (Preset::OpenCodeZen, ModelCategory::OpenCodeZen),
    ] {
        assert_eq!(preset.category(), category);
    }
}
