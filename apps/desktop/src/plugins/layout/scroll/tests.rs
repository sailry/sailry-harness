use super::*;
use core::prelude::v1::test;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_shell::policy::{self, Policy};

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

fn wheel(visual: &mut VisualTestContext, dy: f32) {
    visual.simulate_event(ScrollWheelEvent {
        position: point(px(50.), px(50.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
        ..Default::default()
    });
    draw(visual);
}

fn marker(id: &'static str, height: f32) -> Div {
    div()
        .debug_selector(move || id.into())
        .h(px(height))
        .flex_shrink_0()
}

struct Nested {
    overflow: bool,
}

impl Render for Nested {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(160.))
            .h(px(200.))
            .v_flex()
            .overflow_y_scrollbar()
            .child(
                div()
                    .w_full()
                    .h(px(100.))
                    .flex_shrink_0()
                    .v_flex()
                    .overflow_y_scrollbar()
                    .child(marker(
                        "inner-first",
                        if self.overflow { 150. } else { 30. },
                    ))
                    .child(marker("inner-last", if self.overflow { 150. } else { 30. })),
            )
            .child(marker("outer-last", 300.))
    }
}

#[gpui::test]
fn kit_regions_chain_only_at_vertical_edges(cx: &mut TestAppContext) {
    crate::plugins::tests::init(cx);
    for overflow in [true, false] {
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|_| Nested { overflow });
            Root::new(view, window, cx)
        });
        draw(visual);
        let outer = visual.debug_bounds("outer-last").unwrap();
        let inner = visual.debug_bounds("inner-first").unwrap();
        wheel(visual, -40.);
        assert_eq!(
            visual.debug_bounds("inner-first").unwrap().top(),
            inner.top() - px(40.)
        );
        if overflow {
            assert_eq!(visual.debug_bounds("outer-last").unwrap(), outer);
            wheel(visual, -1000.);
            assert_eq!(visual.debug_bounds("outer-last").unwrap(), outer);
            assert_eq!(
                visual.debug_bounds("inner-last").unwrap().bottom(),
                px(100.)
            );
            wheel(visual, -40.);
            assert_eq!(
                visual.debug_bounds("outer-last").unwrap().top(),
                outer.top() - px(40.)
            );
            wheel(visual, 1000.);
            assert_eq!(
                visual.debug_bounds("outer-last").unwrap().top(),
                outer.top() - px(40.)
            );
            wheel(visual, 40.);
            assert_eq!(visual.debug_bounds("outer-last").unwrap(), outer);
            assert_eq!(visual.debug_bounds("inner-first").unwrap(), inner);
        } else {
            assert_eq!(
                visual.debug_bounds("outer-last").unwrap().top(),
                outer.top() - px(40.)
            );
        }
        visual.update(|window, _| window.remove_window());
    }
}

struct DefaultPolicy(Option<Policy>);
impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

#[gpui::test]
fn script_region_chains_and_retains_position_on_refresh(cx: &mut TestAppContext) {
    crate::plugins::tests::init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View, div} from 'gpui-kit';
import {ScrollRegion, Probe} from 'sailry/test';
export default class Regions extends View {
  render() {
    return div().w(160).h(200).v_flex().overflow_y_scrollbar()
      .child(ScrollRegion.new('retained-inner',{max_height:100})
        .child(Probe.new('inner-first',{height:150}))
        .child(Probe.new('inner-last',{height:150})))
      .child(Probe.new('outer-last',{height:300}));
  }
}
"#,
    )
    .unwrap();
    let module = extend(HostModule::new("sailry/test"));
    let declarations = format!(
        "{}\nexport const Probe: {{ new(id: string, props: {{height: number}}): import('gpui-kit').Element }};",
        module.declared().unwrap()
    );
    let module = module
        .component("Probe", |args, _, _| {
            let id = args.id().to_owned();
            let height = args
                .props()
                .get("height")
                .and_then(HostValue::as_number)
                .unwrap() as f32;
            div()
                .debug_selector(move || id.clone())
                .h(px(height))
                .flex_shrink_0()
                .into_any_element()
        })
        .declarations(declarations);
    let granted = Policy::new()
        .with_capabilities(
            gpui_shell::Capabilities::new().read_roots([directory.path().to_path_buf()]),
        )
        .with_host_module(module)
        .unwrap();
    let mut previous = None;
    policy::update_default(|current| {
        previous = Some(current);
        granted
    });
    let _restore = DefaultPolicy(previous);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let mut application = None;
    let mut script = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = runtime.try_load(directory.path(), window, cx).unwrap();
        let content = view
            .read(cx)
            .content()
            .clone()
            .downcast::<gpui_shell::ScriptView>()
            .unwrap();
        assert_eq!(content.read(cx).build_error(), None);
        script = Some(content.clone());
        application = Some(view);
        Root::new(content, window, cx)
    });
    let _application = application.unwrap();
    let script = script.unwrap();
    draw(visual);
    let outer = visual.debug_bounds("outer-last").unwrap();
    let first = visual.debug_bounds("inner-first").unwrap();
    wheel(visual, -40.);
    assert_eq!(visual.debug_bounds("outer-last").unwrap(), outer);
    let moved = visual.debug_bounds("inner-first").unwrap();
    assert_eq!(moved.top(), first.top() - px(40.));
    script.update(visual, |_, cx| cx.notify());
    draw(visual);
    assert_eq!(visual.debug_bounds("inner-first").unwrap(), moved);
    wheel(visual, -1000.);
    assert_eq!(visual.debug_bounds("outer-last").unwrap(), outer);
    assert_eq!(
        visual.debug_bounds("inner-last").unwrap().bottom(),
        px(100.)
    );
    wheel(visual, -40.);
    assert_eq!(
        visual.debug_bounds("outer-last").unwrap().top(),
        outer.top() - px(40.)
    );
    wheel(visual, 1000.);
    wheel(visual, 40.);
    assert_eq!(visual.debug_bounds("outer-last").unwrap(), outer);
    assert_eq!(visual.debug_bounds("inner-first").unwrap(), first);
    visual.update(|window, _| window.remove_window());
}
