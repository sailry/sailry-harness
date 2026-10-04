use super::*;
use crate::plugins::tests::{click, init};
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
    visual.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

#[gpui::test]
fn preserves_slots_and_native_expansion(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View,div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {Collapse,CollapseSlot,collapseOpen,Anchor,record} from 'sailry/test';
export default class Groups extends View {
  shown = true;
  render() {
    return div().v_flex().w_full().p_4().gap_3()
      .child(Anchor.new('visibility').child(new Button('visibility').label('Visibility')
        .on_click((_,cx)=>{record(collapseOpen('node-b'));this.shown=!this.shown;cx.notify();})))
      .children((this.shown?['node-a','node-b']:[]).map(id=>Collapse.new(id,{label:id,default_open:true,variant:id==='node-b'?'plain':'panel'})
        .child(CollapseSlot.new(`${id}-body`,{variant:'content'}).child(div().h(48).line_height('1rem').child('Content')))
        .child(CollapseSlot.new(`${id}-actions`,{variant:'actions'})
          .child(Anchor.new(`${id}-action`).child(new Button(`${id}-action`).ghost().size('small').label('Action')
            .on_click(()=>record()))))
        .child(CollapseSlot.new(`${id}-title`,{variant:'header'}).child(div().child(id)))))
      .child(Collapse.new('duplicate',{label:'Invalid'})
        .child(CollapseSlot.new('duplicate-first',{variant:'header'}).child('First'))
        .child(CollapseSlot.new('duplicate-second',{variant:'header'}).child('Second'))
        .child(CollapseSlot.new('duplicate-body',{variant:'content'}).child('Invalid')))
      .child(Collapse.new('unknown',{label:'Invalid'})
        .child(CollapseSlot.new('unknown-title',{variant:'header'}).child('Header'))
        .child(CollapseSlot.new('unknown-body',{variant:'content'}).child('Body'))
        .child(CollapseSlot.new('unknown-extra',{variant:'unknown'}).child('Invalid')))
      .child(Collapse.new('missing',{label:'Invalid'})
        .child(CollapseSlot.new('missing-title',{variant:'header'}).child('Invalid')));
  }
}
"#,
    )
    .unwrap();
    let events = Rc::new(Cell::new(0));
    let records = events.clone();
    let choices = Rc::new(RefCell::new(Vec::new()));
    let selected = choices.clone();
    let module = HostModule::new("sailry/test")
        .component("Anchor", |mut args, _, _| {
            let id = args.id().to_owned();
            div()
                .debug_selector(move || id.clone())
                .children(args.take_children())
                .into_any_element()
        })
        .function("record", move |args| {
            if let Ok(value) = args.boolean(0) {
                selected.borrow_mut().push(value);
                return Ok(HostValue::Null);
            }
            records.set(records.get() + 1);
            Ok(HostValue::Null)
        })
        .declarations(
            r#"
                export const Anchor: { new(id: string): import("gpui-kit").Element };
                export function record(value?: boolean): void;
            "#,
        );
    let module = extend(module);
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
    draw(visual);
    for id in ["node-a", "node-b"] {
        let header = visual.debug_bounds(id).unwrap();
        let body = visual
            .debug_bounds(Box::leak(format!("{id}-body").into_boxed_str()))
            .unwrap();
        assert!(header.bottom() <= body.top());
        assert_eq!(body.size.height, px(48.));
    }
    assert_eq!(
        visual.update(|window, cx| window.notifications(cx).len()),
        3
    );
    assert!(visual.debug_bounds("duplicate-first").is_none());
    assert!(visual.debug_bounds("unknown-title").is_none());
    assert!(visual.debug_bounds("missing-title").is_none());
    click(visual, "node-a");
    draw(visual);
    assert!(visual.debug_bounds("node-a-body").is_none());
    assert!(visual.debug_bounds("node-a-title").is_some());
    assert!(visual.debug_bounds("node-b-body").is_some());
    visual.simulate_event(MouseMoveEvent {
        position: point(px(1.), px(1.)),
        ..Default::default()
    });
    // Kit does not focus a clicked button; seed its ordinary keyboard scope.
    visual.update(|window, cx| window.focus_next(cx));
    draw(visual);
    for _ in 0..4 {
        visual.simulate_keystrokes("tab");
        draw(visual);
        if visual.debug_bounds("node-a-actions-focused").is_some() {
            break;
        }
    }
    assert!(visual.debug_bounds("node-a-actions-focused").is_some());
    let keystroke = Keystroke::parse("enter").unwrap();
    visual.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    visual.simulate_event(KeyUpEvent { keystroke });
    draw(visual);
    assert_eq!(events.get(), 1);
    assert!(visual.debug_bounds("node-a-body").is_none());
    click(visual, "node-b");
    draw(visual);
    assert!(visual.debug_bounds("node-b-body").is_none());
    click(visual, "node-a");
    draw(visual);
    assert!(visual.debug_bounds("node-a-body").is_some());
    assert!(visual.debug_bounds("node-b-body").is_none());
    click(visual, "visibility");
    draw(visual);
    assert!(visual.debug_bounds("node-a-title").is_none());
    assert!(visual.debug_bounds("node-b-title").is_none());
    click(visual, "visibility");
    draw(visual);
    assert!(visual.debug_bounds("node-a-body").is_some());
    assert!(visual.debug_bounds("node-b-title").is_some());
    assert!(visual.debug_bounds("node-b-body").is_none());
    assert_eq!(&*choices.borrow(), &[false, false]);
    visual.update(|window, _| window.remove_window());
}
