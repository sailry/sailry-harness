use super::*;
use core::prelude::v1::test;
use gpui_shell::policy::{self, Policy};
use std::{cell::Cell, rc::Rc};

struct DefaultPolicy(Option<Policy>);
impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn hover_swaps_metadata_without_moving_the_card(cx: &mut TestAppContext) {
    crate::plugins::tests::init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../plugins/progress/dev.sailry.platform/desktop/state.js"),
        directory.path().join("state.js"),
    )
    .unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View, div} from 'gpui-kit';
import {CardButton, HoverSwap, ThinkingIcon, Paint} from 'sailry/test';
import {clock} from './state.js';
export default class Cards extends View {
  render() {
    return div().w(360).p_4().child(CardButton.new('card',{label:'Card',tooltip:'Running'})
      .child(div().h_flex().w_full().gap_2()
        .child(ThinkingIcon.new('loading',{phase:'turn_tools_running'}))
        .child(div().flex_1().child('Title'))
        .child(HoverSwap.new('metadata',{group:'card'})
          .child(Paint.new('project'))
          .child(Paint.new('time',{text:clock(new Date(2026,9,3,21,7).getTime())})))));
  }
}
"#,
    )
    .unwrap();
    let project = Rc::new(Cell::new(0));
    let time = Rc::new(Cell::new(0));
    let module = module(HostModule::new("sailry/test"), CancellationToken::new());
    let declarations = format!(
        "{}\nexport const Paint: {{ new(id: string, props?: {{text: string}}): import('gpui-kit').Element }};",
        module.declared().unwrap()
    );
    let module = module
        .component("Paint", {
            let project = project.clone();
            let time = time.clone();
            move |args, _, _| {
                if args.id() == "time" {
                    assert_eq!(string(args.props(), "text"), "21:07");
                }
                let painted = if args.id() == "project" {
                    project.clone()
                } else {
                    time.clone()
                };
                div()
                    .size_full()
                    .child(canvas(
                        |_, _, _| (),
                        move |_, _, _, _| painted.set(painted.get() + 1),
                    ))
                    .into_any_element()
            }
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
    visual.simulate_mouse_move(point(px(600.), px(300.)), None, Modifiers::default());
    draw(visual);
    let card = visual.debug_bounds("card").unwrap();
    let metadata = visual.debug_bounds("metadata").unwrap();
    assert!(project.get() > 0);
    assert_eq!(time.get(), 0);
    project.set(0);
    time.set(0);
    visual.simulate_mouse_move(card.center(), None, Modifiers::default());
    draw(visual);
    assert_eq!(visual.debug_bounds("card").unwrap(), card);
    assert_eq!(visual.debug_bounds("metadata").unwrap(), metadata);
    assert_eq!(project.get(), 0);
    assert!(time.get() > 0);
    project.set(0);
    time.set(0);
    visual.simulate_mouse_move(point(px(600.), px(300.)), None, Modifiers::default());
    draw(visual);
    assert_eq!(visual.debug_bounds("card").unwrap(), card);
    assert!(project.get() > 0);
    assert_eq!(time.get(), 0);
    visual.update(|window, _| window.remove_window());
}
