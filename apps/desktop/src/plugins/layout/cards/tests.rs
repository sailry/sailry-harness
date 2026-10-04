use super::*;
use crate::plugins::{
    controls,
    tests::{click, init, wait},
};
use core::prelude::v1::test;
use gpui_kit::{component::*, *};
use gpui_shell::policy::{self, Policy};
use sailry_link::CancellationToken;
use std::{cell::RefCell, rc::Rc};

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        policy::set_default(self.0.take().unwrap());
    }
}

fn selector(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[gpui::test]
fn preserves_layout_and_native_callbacks(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("main.js"),
        r#"
import {View,div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {CardList,CardRow,CardColumn,CardSummary,Toggle,nextControlEvent} from 'sailry/ui';
import {Anchor,record} from 'sailry/test';
const long='A long label that must remain within its column 中文 🙂 '.repeat(12);
function button(id,label,disabled=false) {
  return Anchor.new(id).child(new Button(id).size('small').ghost().label(label).disabled(disabled)
    .on_click(()=>record(id,'clicked')));
}
export default class Cards extends View {
  init(_props,cx) {
    this.checked={checkbox:false,switch:false};
    cx.spawn(async cx=>{
      while(true) {
        let event;
        try {event=await nextControlEvent();} catch {return;}
        this.checked[event.id]=event.value;
        record(event.id,String(event.value));cx.notify();
      }
    });
  }
  item(kind) {
    const toggle=Toggle.new(kind,{variant:kind,label:'Enabled',checked:this.checked[kind]});
    return CardRow.new(`${kind}-card`,{row_id:`${kind}-row`})
      .child(CardSummary.new(`${kind}-summary`,{title:long,subtitle:long,icon:'icons/folder.svg'})
        .children(kind==='checkbox'?[toggle]:[]))
      .child(CardColumn.new(`${kind}-metadata`,{variant:kind==='checkbox'?'wide_metadata':'metadata'}).child(long))
      .children(kind==='switch'?[CardColumn.new(`${kind}-control`,{variant:'control'}).child(toggle)]:[])
      .child(CardColumn.new(`${kind}-actions`,{variant:'actions'})
        .child(button(`${kind}-edit`,'Edit')).child(button(`${kind}-remove`,'Remove',true)));
  }
  render() {
    return div().size_full().v_flex().items_center().p_4()
      .child(div().w_full().max_w(800).min_w_0()
        .child(CardList.new('cards').child(this.item('checkbox')).child(this.item('switch'))
          .child(CardRow.new('queue-card')
            .child(CardColumn.new('queue-content').child(div().truncate().child(long)))
            .child(CardColumn.new('queue-label',{variant:'inline'}).child('Concurrency'))
            .child(CardColumn.new('queue-actions',{variant:'actions',spacing:'regular'})
              .child(button('queue-less','−'))
              .child(Anchor.new('queue-count').child(div().child('2')))
              .child(button('queue-more','+'))))));
  }
}
"#,
    )
    .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let records = events.clone();
    let anchors = HostModule::new("sailry/test")
        // Only controls need probes; card geometry uses its native selectors.
        .component("Anchor", |mut args, _, _| {
            div()
                .debug_selector(|| args.id().to_owned())
                .children(args.take_children())
                .into_any_element()
        })
        .function("record", move |args| {
            records
                .borrow_mut()
                .push((args.string(0)?.to_owned(), args.string(1)?.to_owned()));
            Ok(HostValue::Null)
        });
    let stop = CancellationToken::new();
    let ui = cx
        .update(|cx| {
            controls::module(
                extend(HostModule::new("sailry/ui")),
                stop.clone(),
                controls::tabs::Controls::new(stop.clone()),
                cx,
            )
        })
        .function("selectSettingsHost", |_| {
            // The shared catalog requires this entry, but this fixture has no Node.
            Err(gpui_shell::HostError::new(
                "host selection is unavailable in the isolated layout fixture",
            ))
        });
    let granted = Policy::new()
        .with_capabilities(
            gpui_shell::Capabilities::new().read_roots([directory.path().to_path_buf()]),
        )
        .with_host_module(ui)
        .unwrap()
        .with_host_module(anchors)
        .unwrap();
    let mut previous = None;
    policy::update_default(|current| {
        previous = Some(current);
        granted
    });
    let _restore = DefaultPolicy(previous);
    let runtime = gpui_component_shell::new_isolated_runtime().unwrap();
    let mut application = None;
    let mut script = None;
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
    let mut layouts = Vec::new();
    let mut checked = false;
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        for (index, width) in [1000., 640., 480.].into_iter().enumerate() {
            let handle = visual.update(|window, cx| {
                Theme::change(mode, Some(window), cx);
                window.window_handle()
            });
            visual.simulate_window_resize(handle, size(px(width), px(600.)));
            visual.run_until_parked();
            visual.update(|window, cx| {
                window.draw(cx).clear(cx);
                assert_eq!(script.read(cx).build_error(), None);
            });
            let list = visual.debug_bounds("cards").unwrap();
            assert_eq!(list.size.width, px(800_f32.min(width - 32.)));
            assert!(list.left() >= px(16.) && list.right() <= px(width - 16.));
            let mut geometry = vec![list];
            let mut previous = None;
            for kind in ["checkbox", "switch", "queue"] {
                let card = visual
                    .debug_bounds(selector(format!("{kind}-card")))
                    .unwrap();
                let row = visual
                    .debug_bounds(if kind == "queue" {
                        "queue-card-row"
                    } else {
                        selector(format!("{kind}-row"))
                    })
                    .unwrap();
                assert_eq!(card.size.width, list.size.width);
                assert!(card.left() >= list.left() && card.right() <= list.right());
                if kind == "queue" {
                    assert!(row.size.height <= px(64.), "{kind}: {row:?}");
                } else {
                    assert!(
                        row.size.height >= px(64.) && row.size.height <= px(72.),
                        "{kind}: {row:?}"
                    );
                }
                if let Some(bottom) = previous {
                    assert!((card.top() - bottom - px(12.)).abs() <= px(1.));
                }
                previous = Some(card.bottom());
                geometry.extend([card, row]);
                let columns: &[&str] = match kind {
                    "queue" => &["content", "label", "actions"],
                    "checkbox" => &["summary", "metadata", "actions"],
                    _ => &["summary", "metadata", "control", "actions"],
                };
                let mut right = row.left();
                for (index, part) in columns.iter().enumerate() {
                    let column = visual
                        .debug_bounds(selector(format!("{kind}-{part}")))
                        .unwrap();
                    assert!(
                        column.left() >= right && column.right() <= row.right(),
                        "{kind}-{part}: {column:?}, row: {row:?}"
                    );
                    assert!((column.center().y - row.center().y).abs() <= px(1.));
                    assert!(
                        (column.left() - right - px(if index == 0 { 0. } else { 12. })).abs()
                            <= px(1.)
                    );
                    geometry.push(column);
                    right = column.right();
                }
                if kind != "queue" {
                    let summary = visual
                        .debug_bounds(selector(format!("{kind}-summary")))
                        .unwrap();
                    let title = visual
                        .debug_bounds(selector(format!("{kind}-summary-title")))
                        .unwrap();
                    let subtitle = visual
                        .debug_bounds(selector(format!("{kind}-summary-subtitle")))
                        .unwrap();
                    let icon = visual
                        .debug_bounds(selector(format!("{kind}-summary-icon")))
                        .unwrap();
                    assert!(title.bottom() < subtitle.top());
                    assert_eq!(title.left(), subtitle.left());
                    assert!(title.left() >= summary.left() && title.right() <= summary.right());
                    assert!(subtitle.right() <= summary.right());
                    assert_eq!(icon.size, size(px(16.), px(16.)));
                    assert_eq!(
                        visual
                            .debug_bounds(selector(format!("{kind}-metadata")))
                            .unwrap()
                            .size
                            .width,
                        px(if kind == "checkbox" { 192. } else { 128. })
                    );
                    let control = visual.debug_bounds(kind).unwrap();
                    assert!((control.center().y - summary.center().y).abs() <= px(1.));
                    if kind == "checkbox" {
                        assert!(control.left() >= summary.left() && control.right() < title.left());
                        assert!((title.left() - control.right() - px(12.)).abs() <= px(1.));
                    } else {
                        let lane = visual.debug_bounds("switch-control").unwrap();
                        assert_eq!(lane.size.width, px(48.));
                        assert!((control.center().x - lane.center().x).abs() <= px(1.));
                    }
                }
            }
            assert!(
                (list.bottom() - previous.unwrap()).abs() <= px(1.),
                "the list must retain its content height"
            );
            let less = visual.debug_bounds("queue-less").unwrap();
            let count = visual.debug_bounds("queue-count").unwrap();
            let more = visual.debug_bounds("queue-more").unwrap();
            assert!((count.left() - less.right() - px(12.)).abs() <= px(1.));
            assert!((more.left() - count.right() - px(12.)).abs() <= px(1.));
            if mode == ThemeMode::Light {
                layouts.push(geometry);
            } else {
                assert_eq!(geometry, layouts[index]);
            }

            checked = !checked;
            for kind in ["checkbox", "switch"] {
                click(visual, kind);
                wait(visual, |_| events.borrow().len() == 1);
                assert_eq!(
                    events.borrow_mut().pop(),
                    Some((kind.into(), checked.to_string()))
                );
                click(visual, selector(format!("{kind}-edit")));
                assert_eq!(
                    events.borrow_mut().pop(),
                    Some((format!("{kind}-edit"), "clicked".into()))
                );
                click(visual, selector(format!("{kind}-remove")));
                click(visual, selector(format!("{kind}-summary-title")));
                assert!(events.borrow().is_empty());
            }
            for id in ["queue-less", "queue-more"] {
                click(visual, id);
                assert_eq!(
                    events.borrow_mut().pop(),
                    Some((id.into(), "clicked".into()))
                );
                assert!(events.borrow().is_empty());
            }
        }
    }
    stop.cancel();
    visual.update(|window, _| window.remove_window());
}
