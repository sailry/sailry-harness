use super::*;
use crate::settings::providers::{editor::open, vendors::Vendor};
use core::prelude::v1::test;

struct Surface(Entity<Workspace>);

impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn tap(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

fn setup(cx: &mut TestAppContext) -> (Entity<Editor>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_reduce_motion(true);
    });
    let mut workspace = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let owner = cx.new(|cx| Workspace::new(window, cx));
        workspace = Some(owner.clone());
        let surface = cx.new(|_| Surface(owner));
        Root::new(surface, window, cx)
    });
    let editor = visual.update(|window, cx| open(workspace.unwrap(), None, window, cx));
    draw(visual);
    (editor, visual)
}

#[gpui::test]
fn folds_and_selects(cx: &mut TestAppContext) {
    let (editor, visual) = setup(cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for (width, height) in [(1280., 820.), (760., 560.)] {
            let handle = visual.update(|window, cx| {
                crate::theme::select(Some(mode), window, cx);
                editor.update(cx, |editor, cx| {
                    editor.select_preset(Preset::OpenAi, window, cx)
                });
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(height)));
            draw(visual);
            tap(visual, "provider-type");
            assert!(visual.debug_bounds("provider-kind-preset_openai").is_some());
            assert!(
                visual
                    .debug_bounds("provider-kind-preset_anthropic")
                    .is_none()
            );
            let expanded = visual.debug_bounds("provider-kind-picker").unwrap();
            assert!(expanded.left() >= px(0.) && expanded.right() <= px(width));
            assert!(expanded.top() >= px(0.) && expanded.bottom() <= px(height));
            tap(visual, "provider-kind-brand-provider_openai");
            assert!(visual.debug_bounds("provider-kind-preset_openai").is_none());
            assert!(
                visual
                    .debug_bounds("provider-kind-picker")
                    .unwrap()
                    .size
                    .height
                    <= expanded.size.height
            );
            tap(visual, "provider-kind-brand-provider_anthropic");
            assert!(
                visual
                    .debug_bounds("provider-kind-preset_anthropic_compatible")
                    .is_some()
            );
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.preset),
                Preset::OpenAi
            );
            tap(visual, "provider-kind-preset_anthropic_compatible");
            assert_eq!(
                editor.read_with(visual, |editor, _| editor.preset),
                Preset::CompatibleAnthropic
            );
            assert!(visual.debug_bounds("provider-kind-picker").is_none());
            tap(visual, "provider-type");
            assert!(
                visual
                    .debug_bounds("provider-kind-preset_anthropic_compatible")
                    .is_some()
            );
            assert!(visual.debug_bounds("provider-kind-preset_openai").is_none());
            visual.simulate_keystrokes("escape");
            draw(visual);
            assert!(visual.debug_bounds("provider-kind-picker").is_none());
            assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        }
    }
}

#[gpui::test]
fn navigates_with_keyboard(cx: &mut TestAppContext) {
    let (editor, visual) = setup(cx);
    tap(visual, "provider-type");
    visual.simulate_keystrokes("up left");
    draw(visual);
    assert!(visual.debug_bounds("provider-kind-preset_openai").is_none());
    visual.simulate_keystrokes("right");
    draw(visual);
    assert!(visual.debug_bounds("provider-kind-preset_openai").is_some());
    visual.simulate_keystrokes("enter");
    draw(visual);
    assert!(visual.debug_bounds("provider-kind-preset_openai").is_none());
    visual.simulate_keystrokes("down right down down down enter");
    draw(visual);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.preset),
        Preset::CompatibleAnthropic
    );
    assert!(visual.debug_bounds("provider-kind-picker").is_none());
    tap(visual, "provider-type");
    for _ in 2..groups().len() {
        visual.simulate_keystrokes("down");
        draw(visual);
    }
    let last = visual
        .debug_bounds("provider-kind-brand-provider_cohere")
        .unwrap();
    let panel = visual.debug_bounds("provider-kind-picker").unwrap();
    assert!(last.top() >= panel.top() && last.bottom() <= panel.bottom());
    visual.simulate_keystrokes("right down enter");
    draw(visual);
    assert_eq!(
        editor.read_with(visual, |editor, _| editor.preset),
        Preset::Hosted(Vendor::Cohere)
    );
    tap(visual, "provider-type");
    assert!(
        visual
            .debug_bounds("provider-kind-provider_cohere")
            .is_some()
    );
    visual.simulate_keystrokes("escape");
    draw(visual);
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn selects_opencode_go(cx: &mut TestAppContext) {
    let (editor, visual) = setup(cx);
    tap(visual, "provider-type");
    tap(visual, "provider-kind-brand-provider_opencode");
    visual.simulate_keystrokes("down enter");
    draw(visual);
    editor.read_with(visual, |editor, cx| {
        assert_eq!(editor.preset, Preset::OpenCodeGo);
        assert_eq!(
            editor.preset.category(),
            super::super::super::super::data::Category::OpenCodeGo
        );
        assert_eq!(
            editor.connection(cx).unwrap(),
            (
                sailry_protocol::conversation::ModelApi::OpenCodeGo,
                "https://opencode.ai/zen/go/v1".into()
            )
        );
    });
    assert!(visual.debug_bounds("provider-kind-picker").is_none());
}

#[gpui::test]
fn selects_plan_and_region(cx: &mut TestAppContext) {
    use sailry_protocol::conversation::{ModelApi, connection::Product};
    let (editor, visual) = setup(cx);
    tap(visual, "provider-type");
    tap(visual, "provider-kind-brand-provider_qwen");
    tap(visual, "provider-kind-provider_qwen");
    assert!(visual.debug_bounds("provider-product").is_some());
    assert!(visual.debug_bounds("provider-region").is_some());
    tap(visual, "provider-product");
    visual.simulate_keystrokes("down enter");
    draw(visual);
    editor.read_with(visual, |editor, cx| {
        assert_eq!(editor.product(), Product::Coding);
        assert_eq!(
            editor.connection(cx).unwrap(),
            (
                ModelApi::ChatCompletions,
                "https://coding.dashscope.aliyuncs.com/v1".into()
            )
        );
    });
    tap(visual, "provider-region");
    visual.simulate_keystrokes("down enter");
    draw(visual);
    editor.read_with(visual, |editor, cx| {
        assert_eq!(
            editor.connection(cx).unwrap().1,
            "https://coding-intl.dashscope.aliyuncs.com/v1"
        );
        assert_eq!(editor.models[0].id.read(cx).value(), "qwen3.6-plus");
    });
    assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn blocks_region_for_unset_product(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use sailry_protocol::conversation::connection::Product;
    let (editor, visual) = setup(cx);
    let endpoint = Vendor::Qwen.coding_endpoints()[1].1;
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_preset(Preset::Hosted(Vendor::Qwen), window, cx);
            editor.hosted_product = None;
            editor
                .endpoint
                .update(cx, |input, cx| input.set_value(endpoint, window, cx));
            editor.sync_product(window, cx);
            editor.sync_region(window, cx);
            cx.notify();
        })
    });
    draw(visual);
    visual.update(|window, _| {
        assert_eq!(
            window.find("provider-product-select").value(),
            Some(tr("provider_product").as_ref())
        );
        assert_eq!(
            window.find("provider-region-select").value(),
            Some(tr("provider_region_global").as_ref())
        );
    });
    tap(visual, "provider-region");
    assert!(visual.debug_bounds("select-popup").is_none());
    editor.read_with(visual, |editor, cx| {
        assert_eq!(editor.hosted_product, None);
        assert_eq!(editor.connection(cx).unwrap().1, endpoint);
    });
    visual.update(|window, cx| {
        editor.update(cx, |editor, cx| {
            editor.select_product(Product::Coding, window, cx)
        })
    });
    draw(visual);
    tap(visual, "provider-region");
    assert!(visual.debug_bounds("select-popup").is_some());
    visual.simulate_keystrokes("up enter");
    draw(visual);
    editor.read_with(visual, |editor, cx| {
        assert_eq!(editor.hosted_product, Some(Product::Coding));
        assert_eq!(
            editor.connection(cx).unwrap().1,
            Vendor::Qwen.coding_endpoints()[0].1
        );
    });
}

#[gpui::test]
fn selects_opencode_zen(cx: &mut TestAppContext) {
    let (editor, visual) = setup(cx);
    tap(visual, "provider-type");
    tap(visual, "provider-kind-brand-provider_opencode");
    visual.simulate_keystrokes("down down enter");
    draw(visual);
    editor.read_with(visual, |editor, cx| {
        assert_eq!(editor.preset, Preset::OpenCodeZen);
        assert_eq!(
            editor.preset.category(),
            super::super::super::super::data::Category::OpenCodeZen
        );
        assert_eq!(
            editor.connection(cx).unwrap(),
            (
                sailry_protocol::conversation::ModelApi::OpenCodeZen,
                "https://opencode.ai/zen/v1".into()
            )
        );
    });
    assert!(visual.debug_bounds("provider-kind-picker").is_none());
}

#[gpui::test]
fn respects_disabled_trigger(cx: &mut TestAppContext) {
    let (editor, visual) = setup(cx);
    for editing in [None, Some(0)] {
        visual.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.editing = editing;
                editor.pending = editing.is_none();
                cx.notify();
            })
        });
        draw(visual);
        tap(visual, "provider-type");
        assert!(visual.debug_bounds("provider-kind-picker").is_none());
    }
}
