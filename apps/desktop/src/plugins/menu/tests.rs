use super::*;
use crate::plugins::tests::{click, init, wait};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use gpui_shell::policy::{self, Policy};
use std::cell::RefCell;

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

#[gpui::test]
fn form_popup_tracks_trigger_width(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("main.js"), r#"
import {View,div} from 'gpui-kit';
import {Menu,Marker,nextMenuEvent} from 'sailry/test';
const items = [
  {id:'short',label:'Short',enabled:true,checked:true},
  {id:'blocked',label:'Unavailable',enabled:false},
  {id:'long',label:'Preserve full option 中文 🙂 with a long label that must not expand a form menu',enabled:true},
];
export default class Fields extends View {
  init(_props,cx) {
    this.picked = ''; this.count = 0;
    cx.spawn(async cx => {
      try { while (true) {
        const event = await nextMenuEvent();
        this.picked = `${event.menu}-${event.id}-${++this.count}`; cx.notify();
      } } catch (_) { /* The fixture owns its menu subscription. */ }
    });
  }
  render() {
    return div().size_full().v_flex().items_start().p_4().gap_4().children([
      div().w(220).child(Menu.new('narrow-form',{label:'Short',form:true,items})),
      div().w(460).child(Menu.new('wide-form',{label:'Short',form:true,items})),
      div().w(460).child(Menu.new('action-menu',{label:'Action',items:[items[0]]})),
      Marker.new('selection',{label:this.picked}),
    ]);
  }
}
"#).unwrap();
    let stop = CancellationToken::new();
    let observed = Rc::new(RefCell::new(String::new()));
    let values = observed.clone();
    let module = module(HostModule::new("sailry/test"), stop.clone()).component(
        "Marker",
        move |args, _, _| {
            let value = super::super::host::sdk::values::decode(args.props()).unwrap();
            let label = value["label"].as_str().unwrap().to_owned();
            *values.borrow_mut() = label.clone();
            div()
                .debug_selector(move || label.clone())
                .into_any_element()
        },
    );
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
    for (index, (selector, width)) in [("narrow-form", 220.), ("wide-form", 460.)]
        .into_iter()
        .enumerate()
    {
        visual.simulate_window_resize(handle, size(px(760. + index as f32 * 520.), px(820.)));
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert_eq!(script.read(cx).build_error(), None);
        });
        click(visual, selector);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let trigger = visual.debug_bounds(selector).unwrap();
        let popup = visual.debug_bounds("popup-menu").unwrap();
        assert_eq!(trigger.size.width, px(width));
        assert_eq!(popup.size.width, trigger.size.width);
        visual.simulate_keystrokes("down down enter");
        visual.run_until_parked();
        let picked = Box::leak(format!("{selector}-long-{}", index + 1).into_boxed_str());
        wait(visual, |_| observed.borrow().as_str() == picked);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(picked).is_some());
        assert!(visual.debug_bounds("popup-menu").is_none());
        click(visual, selector);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            visual.debug_bounds("popup-menu").unwrap().size.width,
            px(width)
        );
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
    }
    click(visual, "action-menu");
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("popup-menu").unwrap().size.width < px(220.));
    visual.simulate_keystrokes("escape");
    stop.cancel();
    visual.update(|window, _| window.remove_window());
}
