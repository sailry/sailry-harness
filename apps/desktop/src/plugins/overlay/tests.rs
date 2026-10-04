use super::*;
use crate::plugins::{
    Panel,
    fixture::Fixture,
    tests::{click, init, snapshot, wait},
};
use core::prelude::v1::test;
use gpui_kit::component::Root;

#[gpui::test]
fn close_controls_preserve_guards_and_restore_focus(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package();
        std::fs::write(
            fixture.directory.path().join("project/package/dev.sailry.platform/desktop/main.js"),
            r#"import {View, div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {Anchor} from 'sailry/test';
import {Modal,Details,modal_closed} from 'sailry/ui';
import {TextField,createText,readText,focusText} from 'sailry/forms';
export default class Overlays extends View {
  init(_props,cx) {
    this.field=createText('Keep the draft',{label:'Draft',placeholder:'Write a draft'});
    this.open=false;this.form=false;this.dismissable=true;this.closed=0;
    cx.spawn(async cx=>{while(true){if(await modal_closed()==='overlay'){this.open=false;this.closed++;cx.notify();}}});
  }
  show(form,cx){focusText(this.field);this.form=form;this.open=true;this.dismissable=false;cx.notify();}
  render(){return div().v_flex().p_4().gap_3()
    .child(TextField.new(this.field))
    .child(`draft:${readText(this.field)}`)
    .child(Anchor.new('open-default').child(new Button('default').label('Open').on_click((_,cx)=>this.show(false,cx))))
    .child(Anchor.new('open-form').child(new Button('form').label('Open form').on_click((_,cx)=>this.show(true,cx))))
    .child(`closed:${this.closed}`)
    .child(`open:${this.open}`)
    .child(`dismissable:${this.dismissable}`)
    .child(Modal.new('overlay',{open:this.open,form:this.form,title:'Edit',width:400,dismissable:this.dismissable})
      .child(div().v_flex().w(352).gap_3().child(Details.new('shared-details',{subtitle:'Resource',content:'Body'}))
        .child(Anchor.new('allow-close').child(new Button('allow').label('Allow close').on_click((_,cx)=>{this.dismissable=true;cx.notify();}))))
      .children(this.form?[div().child('Footer')]:[]));}
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
        wait(visual, |cx| snapshot(&panel, cx).contains("Keep the draft"));
        for (index, trigger) in ["open-default", "open-form"].into_iter().enumerate() {
            click(visual, "field-1");
            let previous = visual.update(|window, cx| window.focused(cx));
            assert!(previous.is_some());
            click(visual, trigger);
            wait(visual, |cx| snapshot(&panel, cx).contains("open:true"));
            let duration = visual.update(|_, cx| cx.theme().motion_tokens().duration_normal);
            visual.executor().advance_clock(duration);
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            assert!(visual.debug_bounds("overlay-close").is_some());
            assert!(visual.debug_bounds("details-content").is_some());
            assert_ne!(visual.update(|window, cx| window.focused(cx)), previous);
            click(visual, "overlay-close");
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert!(snapshot(&panel, cx).contains(&format!("closed:{index}")));
            });
            assert!(visual.debug_bounds("allow-close").is_some());
            click(visual, "allow-close");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains("dismissable:true")
            });
            click(visual, "overlay-close");
            wait(visual, |cx| {
                snapshot(&panel, cx).contains(&format!("closed:{}", index + 1))
            });
            assert!(visual.debug_bounds("overlay-close").is_none());
            assert_eq!(visual.update(|window, cx| window.focused(cx)), previous);
            visual.update(|_, cx| assert!(snapshot(&panel, cx).contains("Keep the draft")));
        }
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
