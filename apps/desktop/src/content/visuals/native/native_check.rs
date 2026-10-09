//! Actual bundled WebKit SVG rendering in isolated native windows, without screenshots.
use super::*;
use gpui_kit::{
    component::{Root, Theme, ThemeMode},
    *,
};
use sailry_content::markdown::{
    State, View,
    visuals::{Kind, Svg},
};
use std::{cell::RefCell, path::Path, time::Instant};
use wry::WebViewExtMacOS;

const SOURCE: &str = "```mermaid\nflowchart LR\nA[Alpha] --> B[Beta]\n```\n\nInline $x^2 + \\alpha$ formula\n\n$$\n\\frac{1}{2}+\\sum_{n=1}^{3}n\n$$\n";
const INVALID: &str = "flowchart !!! broken";

struct Record {
    request: Request,
    result: Option<Result<Svg, String>>,
}
type Records = Rc<RefCell<Vec<Record>>>;

struct Harness(Entity<State>);
impl gpui_kit::Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().child(View::new(&self.0))
    }
}

pub(in crate::content::visuals) fn run(output: &Path) {
    let output = output.to_owned();
    gpui_kit::application().with_assets(crate::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
            let mut preferences = crate::preferences::Preferences::default();
            preferences.data.appearance.mode = crate::theme::Mode::Light;
            cx.set_global(preferences);
        crate::theme::init(cx);
        crate::content::init(cx);
        Theme::change(ThemeMode::Light, None, cx);
        let records = Rc::new(RefCell::new(Vec::new()));
        trace(records.clone(), cx);
        let output = output.clone();
        cx.spawn(async move |cx| {
            let first_state = Rc::new(RefCell::new(None));
            let state = first_state.clone();
            let first = cx.open_window(WindowOptions::default(), move |window, cx| {
                window.set_window_title("Sailry — native visual fixture");
                let markdown = cx.new(|cx| State::new(SOURCE, cx));
                *state.borrow_mut() = Some(markdown.clone());
                let harness = cx.new(|_| Harness(markdown));
                cx.new(|cx| Root::new(harness, window, cx))
            }).unwrap();
            complete(first, &records, 3, cx).await;
            verify(&records.borrow()[..3]);
            let initial = first.update(cx, |_, _, cx| {
                let surface = cx.global::<Service>().0.read(cx);
                let page = surface.view.as_ref().unwrap();
                assert!(page.webview().isHidden(), "SVG preparation must remain hidden");
                assert_eq!(surface.parent, Some(first.into()));
                surface.generation
            }).unwrap();
            for _ in 0..5 { pause(first, cx).await; }
            assert_eq!(records.borrow().len(), 3, "unchanged frames reuse prepared visuals");
            first.update(cx, |_, window, cx| Theme::change(ThemeMode::Dark, Some(window), cx)).unwrap();
            complete(first, &records, 6, cx).await;
            verify(&records.borrow()[3..6]);
            for index in 0..3 {
                let records = records.borrow();
                let light = &records[index].request;
                let dark = records[3..6].iter().find(|entry| entry.request.kind == light.kind).unwrap();
                assert!(!light.dark && dark.request.dark);
                assert_ne!(light.foreground, dark.request.foreground);
            }
            for _ in 0..5 { pause(first, cx).await; }
            assert_eq!(records.borrow().len(), 6, "theme frames retain their prepared resources");
            let markdown = first_state.borrow().clone().unwrap();
            first.update(cx, |_, _, cx| {
                markdown.update(cx, |state, cx| state.set_source(format!("```mermaid\n{INVALID}\n```"), cx));
            }).unwrap();
            complete(first, &records, 7, cx).await;
            assert!(records.borrow()[6].result.as_ref().unwrap().is_err(), "malformed Mermaid must fail honestly");
            first.update(cx, |_, _, cx| {
                markdown.update(cx, |state, cx| {
                    state.select_all(cx);
                    assert!(state.selected_text().contains(INVALID), "render failure retains the original selectable block");
                });
            }).unwrap();
            pause(first, cx).await;
            assert_eq!(records.borrow().len(), 7, "invalid source is not automatically retried");
            first.update(cx, |_, window, cx| {
                super::release(window, cx);
                let surface = cx.global::<Service>().0.read(cx);
                assert!(surface.view.is_none() && surface.parent.is_none());
                assert!(surface.active.is_none() && surface.queue.is_empty());
                window.remove_window();
            }).unwrap();
            drop(markdown);
            let second = cx.open_window(WindowOptions::default(), |window, cx| {
                window.set_window_title("Sailry — rebuilt visual fixture");
                let markdown = cx.new(|cx| State::new(SOURCE, cx));
                let harness = cx.new(|_| Harness(markdown));
                cx.new(|cx| Root::new(harness, window, cx))
            }).unwrap();
            complete(second, &records, 10, cx).await;
            verify(&records.borrow()[7..10]);
            second.update(cx, |_, window, cx| {
                let surface = cx.global::<Service>().0.read(cx);
                assert_eq!(surface.generation, initial + 1);
                assert_eq!(surface.parent, Some(second.into()));
                assert!(surface.view.as_ref().unwrap().webview().isHidden());
                super::release(window, cx);
                window.remove_window();
            }).unwrap();
            std::fs::write(output.join("markdown-visuals.json"), serde_json::to_vec(&serde_json::json!({
                "native_webkit": true, "offline_svg": true, "mermaid": true,
                "inline_math": true, "display_math": true, "hidden": true,
                "cache": true, "theme": true, "raw_fallback": true,
                "parent_release": true, "parent_rebuild": true,
                "requests": records.borrow().len()
            })).unwrap()).unwrap();
            println!("Native visuals: bundled Mermaid and formulas, hidden SVG preparation, cache, theme, raw fallback and parent recreation verified");
            cx.update(|cx| cx.quit());
        }).detach();
    });
}

fn trace(records: Records, cx: &mut App) {
    let renderer = cx.global::<Renderer>().clone();
    cx.set_global(Renderer(Rc::new(move |request, window, cx| {
        let index = records.borrow().len();
        records.borrow_mut().push(Record {
            request: request.clone(),
            result: None,
        });
        let task = (renderer.0)(request, window, cx);
        let records = records.clone();
        cx.spawn(async move |_| {
            let result = task.await;
            records.borrow_mut()[index].result = Some(result.clone());
            result
        })
    })));
}

async fn pause(window: WindowHandle<Root>, cx: &mut AsyncApp) {
    cx.background_executor()
        .timer(Duration::from_millis(100))
        .await;
    AnyWindowHandle::from(window)
        .update(cx, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })
        .unwrap();
}

async fn complete(
    window: WindowHandle<Root>,
    records: &Records,
    expected: usize,
    cx: &mut AsyncApp,
) {
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        pause(window, cx).await;
        let complete = {
            let records = records.borrow();
            records.len() >= expected
                && records[..expected]
                    .iter()
                    .all(|record| record.result.is_some())
        };
        if complete {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native visual completion deadline: expected {expected}, requested {}",
            records.borrow().len()
        );
    }
    assert_eq!(records.borrow().len(), expected);
}

fn verify(records: &[Record]) {
    assert_eq!(records.len(), 3);
    for kind in [Kind::Mermaid, Kind::InlineMath, Kind::DisplayMath] {
        let record = records
            .iter()
            .find(|record| record.request.kind == kind)
            .unwrap();
        let svg = record
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .expect("actual bundled visual renderer");
        svg.validate().unwrap();
        let document = roxmltree::Document::parse(&svg.source).unwrap();
        match kind {
            Kind::Mermaid => {
                assert!(svg.source.contains("Alpha") && svg.source.contains("Beta"));
                assert!(document.descendants().any(|node| node.has_tag_name("path")));
                assert!(svg.baseline.is_none());
            }
            Kind::InlineMath | Kind::DisplayMath => {
                assert!(
                    document
                        .descendants()
                        .filter(|node| node.has_tag_name("path"))
                        .count()
                        >= 3,
                    "bundled formula glyphs must become local SVG paths"
                );
                assert_eq!(svg.baseline.is_some(), kind == Kind::InlineMath);
                assert!(svg.source.contains(&record.request.foreground));
            }
        }
    }
}
