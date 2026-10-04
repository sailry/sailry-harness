use super::*;
use crate::plugins::{
    Panel,
    fixture::Fixture,
    tests::{click, init, snapshot, wait},
};
use core::prelude::v1::test;
use gpui_kit::component::Root;
use gpui_shell::HostArguments;

#[test]
fn bounds_and_releases_draft_handles() {
    let module = module();
    module.validate().unwrap();
    let arguments = HostArguments::new([HostValue::from("Draft")]);
    let mut handles = Vec::new();
    for _ in 0..MAX_FIELDS {
        handles.push(module.call("createText", &arguments).unwrap());
    }
    assert!(module.call("createText", &arguments).is_err());
    let removed = handles.remove(0);
    module
        .call("releaseText", &HostArguments::new([removed.clone()]))
        .unwrap();
    let next = module.call("createText", &arguments).unwrap();
    assert_ne!(next.as_str(), removed.as_str());
    assert!(
        module
            .call("readText", &HostArguments::new([removed]))
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
    let other = super::module();
    assert!(
        other
            .call("readText", &HostArguments::new([next]))
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
}

#[gpui::test]
fn enter_obeys_field_kind(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    fixture.package();
    std::fs::write(
        fixture.directory.path().join("project/package/dev.sailry.platform/desktop/main.js"),
        r#"import {View, div} from 'gpui-kit';
import {TextField, createText, readText, nextTextEvent} from 'sailry/forms';
export default class Fields extends View {
  init(_props, cx) {
    this.fields = [createText('Query'), createText('Note', {multiline:true})];
    this.events = [[], []];
    cx.spawn(async cx => { while (true) {
      const event = await nextTextEvent(), index = this.fields.indexOf(event.id);
      if (index >= 0 && ['enter', 'change'].includes(event.kind)) this.events[index].push(event.kind);
      cx.notify();
    } });
  }
  render() {
    return div().v_flex().size_full().p_4().gap_4().children(this.fields.map((id, index) => div().v_flex()
      .child(TextField.new(id))
      .child(`events-${index}:${this.events[index].join(',')}`)
      .child(`value-${index}:${readText(id).replace(/\n/g, '<newline>')}`)));
  }
}
"#,
    )
    .unwrap();
    let package = fixture.install(0);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let panel = cx.new(|cx| Panel::standalone(fixture.binding.clone(), cx));
        owner = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = owner.unwrap();
    wait(visual, |cx| {
        panel.read(cx).ready_for(&package.summary.reference(), cx)
    });
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.open(package.summary.reference(), window, cx)
        });
    });
    wait(visual, |cx| snapshot(&panel, cx).contains("value-1:Note"));

    click(visual, "field-1");
    visual.simulate_keystrokes("end");
    visual.simulate_keystrokes("enter");
    wait(visual, |cx| snapshot(&panel, cx).contains("events-0:enter"));
    visual.run_until_parked();
    let tree = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        snapshot(&panel, cx)
    });
    assert!(tree.contains("text \"events-0:enter\""), "{tree}");
    assert!(tree.contains("text \"value-0:Query\""), "{tree}");

    click(visual, "field-2");
    visual.simulate_keystrokes("end");
    visual.simulate_keystrokes("enter");
    wait(visual, |cx| {
        snapshot(&panel, cx).contains("value-1:Note<newline>")
    });
    let tree = visual.update(|window, cx| {
        window.draw(cx).clear(cx);
        snapshot(&panel, cx)
    });
    assert!(tree.contains("text \"events-1:change,enter\""), "{tree}");
    assert!(tree.contains("text \"value-1:Note<newline>\""), "{tree}");
    assert!(tree.contains("text \"events-0:enter\""), "{tree}");
    visual.update(|window, _| window.remove_window());
    drop(panel);
    fixture.close();
}
