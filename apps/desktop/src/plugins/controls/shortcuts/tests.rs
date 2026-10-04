use super::*;
use crate::plugins::tests::{init, wait};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use gpui_shell::policy::{self, Policy};
use std::{cell::Cell, rc::Rc};

struct DefaultPolicy(Option<Policy>);
impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

#[gpui::test]
fn focus_isolates_instances_and_close_revokes_bindings(cx: &mut TestAppContext) {
    init(cx);
    let mut windows = Vec::new();
    for _ in 0..2 {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("main.js"),
            r#"
import {View,div} from 'gpui-kit';
import {registerShortcuts,record} from 'sailry/test';
export default class Scope extends View {
  init(_props,cx) {
    this.handle=cx.focus_handle();
    this.context=registerShortcuts([{keystroke:'ctrl-alt-j',action:'perform'}]);
    this.handle.focus();
  }
  render() {
    return div().size_full().key_context(this.context).track_focus(this.handle)
      .on_action('perform',()=>record());
  }
}
"#,
        )
        .unwrap();
        let stop = CancellationToken::new();
        let calls = Rc::new(Cell::new(0));
        let record = calls.clone();
        let module = cx
            .update(|cx| extend(HostModule::new("sailry/test"), stop.clone(), cx))
            .function("record", move |_| {
                record.set(record.get() + 1);
                Ok(HostValue::Null)
            });
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
            let root = runtime.try_load(directory.path(), window, cx).unwrap();
            let script = root
                .read(cx)
                .content()
                .clone()
                .downcast::<gpui_shell::ScriptView>()
                .unwrap();
            assert_eq!(script.read(cx).build_error(), None);
            application = Some(root);
            Root::new(script, window, cx)
        });
        wait(visual, |_| true);
        windows.push((
            visual.update(|window, _| window.window_handle()),
            application.unwrap(),
            runtime,
            directory,
            stop,
            calls,
        ));
    }
    for index in 0..2 {
        let handle = windows[index].0;
        cx.update(|cx| {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .unwrap()
        });
        cx.dispatch_keystroke(handle, Keystroke::parse("ctrl-alt-j").unwrap());
        cx.run_until_parked();
        assert_eq!(windows[index].5.get(), 1);
        assert_eq!(windows[1 - index].5.get(), usize::from(index == 1));
    }
    cx.update(|cx| {
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&ShellAction::new("perform"))
                .count(),
            2
        );
        crate::shortcuts::save("app.search", Some("secondary-shift-j"), cx).unwrap();
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&ShellAction::new("perform"))
                .count(),
            2
        );
    });
    windows[0].4.cancel();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&ShellAction::new("perform"))
                .count(),
            1
        )
    });
    windows[1].4.cancel();
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            cx.key_bindings()
                .borrow()
                .bindings_for_action(&ShellAction::new("perform"))
                .count(),
            0
        )
    });
    for (handle, ..) in windows {
        cx.update(|cx| {
            handle
                .update(cx, |_, window, _| window.remove_window())
                .unwrap()
        });
    }
}
