use super::*;
use crate::plugins::tests::{click, init};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use gpui_shell::policy::{self, Policy};
use std::cell::Cell;

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

// Observe the actual Kit surface without adding layout or replacing its control.
struct Surface {
    child: AnyElement,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.bounds.set(Some(bounds));
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
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
    let surface = Rc::new(Cell::new(None));
    let mut script = None;
    let mut application = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        gpui_kit::component::surface::set_renderer(
            {
                let surface = surface.clone();
                move |child, _, _| {
                    Surface {
                        child,
                        bounds: surface.clone(),
                    }
                    .into_any_element()
                }
            },
            cx,
        );
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
        surface.set(None);
        click(visual, selector);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let trigger = visual.debug_bounds(selector).unwrap();
        let popup = surface.get().expect("native Select popup surface");
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
        surface.set(None);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(surface.get().is_none(), "selection closes the real popup");

        click(visual, selector);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let reopened = surface.get().expect("native Select popup after selection");
        assert_eq!(visual.debug_bounds(selector).unwrap().size.width, px(width));
        assert_eq!(reopened.size.width, popup.size.width);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        surface.set(None);
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(surface.get().is_none());
        assert!(receiver.try_recv().is_err());
    }
    visual.update(|window, _| window.remove_window());
}
