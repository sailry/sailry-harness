//! Kit's script Button omits selected state and custom-content accessible names.
//! Keep selectable rows and named connection cards on the native Kit button.
use gpui_kit::base::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        ActiveTheme as _, Selectable as _,
        button::{Button, ButtonVariants as _},
        list::ListItem,
    },
    *,
};
use gpui_shell::{HostModule, HostValue};
use serde_json::{Value, json};
use tokio::sync::mpsc::Sender;

pub(super) fn extend(module: HostModule, events: Sender<Value>) -> HostModule {
    module.component("SelectableRow", move |mut args, _, cx| {
        let id = args.id().to_owned();
        let selected = args.props().get("selected").and_then(HostValue::as_bool) == Some(true);
        let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true);
        let stripe = args.props().get("stripe").and_then(HostValue::as_bool);
        let label = args
            .props()
            .get("label")
            .and_then(HostValue::as_str)
            .map(str::to_owned);
        let variant = args.props().get("variant").and_then(HostValue::as_str);
        let events = events.clone();
        if variant == Some("list") {
            return ListItem::new(id.clone())
                .selected(selected)
                .disabled(disabled)
                .rounded_none()
                .px_3()
                .h_8()
                .gap_3()
                .text_sm()
                .border_b_1()
                .border_color(cx.theme().border)
                .text_color(cx.theme().muted_foreground)
                .when_some(stripe, |row, stripe| {
                    row.bg(if stripe {
                        cx.theme().tokens.table_even
                    } else {
                        cx.theme().tokens.table
                    })
                })
                .debug_selector({
                    let id = id.clone();
                    move || id.clone()
                })
                .children(args.take_children())
                .on_click(move |event, _, _| {
                    let _ = events.try_send(json!({"id":id,"click_count":event.click_count()}));
                })
                .into_any_element();
        }
        let button = Button::new(id.clone());
        let button = if variant == Some("card") {
            button.outline().p_3()
        } else {
            button.ghost().px_2()
        };
        button
            .w_full()
            .justify_start()
            .h_auto()
            .selected(selected)
            .disabled(disabled)
            .when_some(label, |button, label| button.accessibility_label(label))
            .debug_selector({
                let id = id.clone();
                move || id.clone()
            })
            .children(args.take_children())
            .on_click(move |_, _, _| {
                let _ = events.try_send(json!({"id":id}));
            })
            .into_any_element()
    })
}

#[cfg(test)]
mod tests {
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
    fn native_list_rows_respect_disabled_state(cx: &mut TestAppContext) {
        init(cx);
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("main.js"), r#"
import {View,div} from 'gpui-kit';
import {SelectableRow} from 'sailry/test';
export default class Rows extends View {
  render() {
    return div().v_flex().w(400).p_4().children([
      SelectableRow.new('disabled-row',{variant:'list',selected:false,disabled:true}).child('Disabled'),
      SelectableRow.new('enabled-row',{variant:'list',selected:false,disabled:false}).child('Enabled'),
    ]);
  }
}
"#).unwrap();
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
            // ShellRoot's cached child does not replay test debug bounds.
            Root::new(content, window, cx)
        });
        let _application = application.unwrap();
        let script = script.unwrap();
        visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            assert_eq!(script.read(cx).build_error(), None);
        });
        click(visual, "disabled-row");
        assert!(receiver.try_recv().is_err());
        click(visual, "enabled-row");
        assert_eq!(
            receiver.try_recv().unwrap(),
            json!({"id":"enabled-row","click_count":1})
        );
        assert!(receiver.try_recv().is_err());
        click(visual, "disabled-row");
        assert!(receiver.try_recv().is_err());
        visual.update(|window, _| window.remove_window());
    }
}
