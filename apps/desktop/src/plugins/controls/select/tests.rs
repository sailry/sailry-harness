use super::*;
use crate::plugins::tests::{click, init};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use gpui_shell::policy::{self, Policy};

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

#[gpui::test]
fn popup_tracks_trigger_and_confirms_options(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View,div} from 'gpui-kit';
import {SelectField} from 'sailry/test';
const items = [
  {id:'short',label:'Short'},
  {id:'long',label:'A long option label that must stay within the control width rather than expanding the popup to its intrinsic text width'},
];
export default class Fields extends View {
  render() {
    return div().size_full().v_flex().items_start().p_4().gap_4().children([
      div().w(220).child(SelectField.new('narrow-select',{label:'Narrow select',items,selected:'short'})),
      div().w(460).child(SelectField.new('wide-select',{label:'Wide select',items,selected:'short'})),
    ]);
  }
}
"#,
    )
    .unwrap();
    let (events, mut receiver) = tokio::sync::mpsc::channel(4);
    let module = extend(HostModule::new("sailry/test"), events);
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
    let mut script = None;
    let mut application = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = runtime.try_load(directory.path(), window, cx).unwrap();
        let content = view
            .read(cx)
            .content()
            .clone()
            .downcast::<gpui_shell::ScriptView>()
            .unwrap();
        script = Some(content.clone());
        application = Some(view);
        Root::new(content, window, cx)
    });
    let _application = application.unwrap();
    let script = script.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(820.)));
    visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        assert_eq!(script.read(cx).build_error(), None);
    });
    for (selector, width) in [("narrow-select", 220.), ("wide-select", 460.)] {
        click(visual, selector);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let trigger = visual.debug_bounds(selector).unwrap();
        let popup = visual
            .debug_bounds("select-popup")
            .expect("native Select popup surface");
        assert_eq!(trigger.size.width, px(width));
        assert!(
            f32::from(popup.size.width - trigger.size.width).abs() <= 1.,
            "Select popup width: trigger={trigger:?}, popup={popup:?}"
        );
        visual.simulate_keystrokes("down");
        visual.simulate_keystrokes("enter");
        visual.run_until_parked();
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"id":selector,"value":"long"})
        );
        assert!(receiver.try_recv().is_err());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(
            visual.debug_bounds("select-popup").is_none(),
            "selection closes the real popup"
        );

        click(visual, selector);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let reopened = visual
            .debug_bounds("select-popup")
            .expect("native Select popup after selection");
        assert_eq!(visual.debug_bounds(selector).unwrap().size.width, px(width));
        assert_eq!(reopened.size.width, popup.size.width);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("select-popup").is_none());
        assert!(receiver.try_recv().is_err());
    }
    visual.update(|window, _| window.remove_window());
}
