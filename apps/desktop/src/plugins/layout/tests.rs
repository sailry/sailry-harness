use super::*;
use core::prelude::v1::test;
use gpui_shell::policy::{self, Policy};

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

#[gpui::test]
fn empty_state_variants(cx: &mut TestAppContext) {
    crate::plugins::tests::init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View, div} from 'gpui-kit';
import {EmptyState} from 'sailry/test';
export default class EmptyStates extends View {
  render() {
    return div().h_flex().items_start().p_4().gap_4().children([
      ['list-auto', 'list', false], ['list-fill', 'list', true],
      ['card-auto', 'card', false], ['card-fill', 'card', true],
      ['list-start', 'list', true, 'start'], ['card-start', 'card', true, 'start'],
    ].map(([id, variant, fill_height, vertical_align]) => div().w(260).h(400).v_flex().flex_shrink_0()
      .child(div().h(40).flex_shrink_0().child('Header'))
      .child(div().v_flex().flex_1().min_h_0()
        .child(EmptyState.new(id, {variant, fill_height, vertical_align, icon:'inbox', label:'No items'})))));
  }
}
"#,
    )
    .unwrap();
    let policy = Policy::new()
        .with_capabilities(
            gpui_shell::Capabilities::new().read_roots([directory.path().to_path_buf()]),
        )
        .with_host_module(extend(HostModule::new("sailry/test")))
        .unwrap();
    let mut previous = None;
    policy::update_default(|current| {
        previous = Some(current);
        policy
    });
    let _restore = DefaultPolicy(previous);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let mut application = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = runtime.try_load(directory.path(), window, cx).unwrap();
        let content = view
            .read(cx)
            .content()
            .clone()
            .downcast::<gpui_shell::ScriptView>()
            .unwrap();
        assert_eq!(content.read(cx).build_error(), None);
        application = Some(view);
        Root::new(content, window, cx)
    });
    let _application = application.unwrap();
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let handle = visual.update(|window, cx| {
            Theme::change(mode, Some(window), cx);
            window.window_handle()
        });
        visual.simulate_window_resize(handle, size(px(1800.), px(600.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        for (id, card, full) in [
            ("list-auto", false, false),
            ("list-fill", false, true),
            ("card-auto", true, false),
            ("card-fill", true, true),
            ("list-start", false, true),
            ("card-start", true, true),
        ] {
            let selector = |prefix: &str| {
                &*Box::leak(format!("{prefix}-{id}").into_boxed_str()) as &'static str
            };
            let body = visual.debug_bounds(selector("empty")).unwrap();
            let surface = visual.debug_bounds(selector("empty-card"));
            assert_eq!(surface.is_some(), card);
            let bounds = surface.unwrap_or(body);
            assert_eq!(bounds.size.width, px(260.));
            if full {
                assert_eq!(bounds.size.height, px(360.));
            } else {
                assert!(bounds.size.height < px(180.));
            }
            let icon = visual.debug_bounds(selector("empty-icon")).unwrap();
            let title = visual.debug_bounds(selector("empty-title")).unwrap();
            if id.ends_with("start") {
                assert!((icon.top() - bounds.top() - px(40.)).abs() <= px(1.));
            } else {
                assert!(
                    ((icon.top() + title.bottom()) / 2. - bounds.center().y).abs() < px(1.),
                    "{id}/{mode:?}: icon={icon:?}, title={title:?}, container={bounds:?}"
                );
            }
        }
    }
    visual.update(|window, _| window.remove_window());
}
