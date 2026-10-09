use super::*;
use crate::conversation::live::tests::fixture::{self, Fixture};
use core::prelude::v1::test;
use sailry_protocol::knowledge::{Definition, Mode, Query, Source};
use std::time::{Duration, Instant};

struct Harness;
impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn indexed(
    fixture: &Fixture,
    bytes: &[u8],
    name: &str,
    query: &str,
) -> (std::path::PathBuf, CitationRequest, CitationPage) {
    let Output::KnowledgeSettings(mut settings) = fixture.execute(Command::ReadKnowledgeSettings)
    else {
        panic!("settings expected")
    };
    settings.model = None;
    fixture.execute(Command::SaveKnowledgeSettings(settings));
    let path = fixture.directory.path().canonicalize().unwrap().join(name);
    std::fs::write(&path, bytes).unwrap();
    let Output::KnowledgeCollection(collection) =
        fixture.execute(Command::CreateKnowledgeCollection {
            definition: Definition {
                name: "Fixture source".into(),
                source: Source::File {
                    path: path.to_str().unwrap().into(),
                },
                model: None,
                globs: Vec::new(),
                tags: Vec::new(),
                shared_with: Vec::new(),
            },
            plugin: None,
        })
    else {
        panic!("collection expected")
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let Output::KnowledgeSearch(page) = fixture.execute(Command::QueryKnowledge(Query {
            query: query.into(),
            collections: vec![collection.id],
            mode: Mode::Keyword,
            tags: Vec::new(),
            path: None,
            modified_after_ms: None,
            modified_before_ms: None,
            limit: 5,
            offset: 0,
        })) else {
            panic!("page expected")
        };
        if let Some(hit) = page.hits.first() {
            let mut input = parse(&hit.citation.link).unwrap();
            input.limit = 128;
            let Output::KnowledgeCitation(page) =
                fixture.execute(Command::ReadKnowledgeCitation(input.clone()))
            else {
                panic!("citation expected")
            };
            return (path, input, page);
        }
        assert!(Instant::now() < deadline, "citation index deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn wait(cx: &mut VisualTestContext, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(cx);
        if cx.update(|_, cx| predicate(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "citation UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn tap(cx: &mut VisualTestContext, selector: &str) {
    use gpui_kit::test::TestWindowExt;

    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    cx.update(|window, cx| {
        let selector = SharedString::from(selector.to_owned());
        assert_ne!(window.find(selector.clone()).disabled(), Some(true));
        window.click(selector, cx);
    });
    draw(cx);
}

#[gpui::test]
fn pages_origin_source_and_retains_stale_context(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let source = format!("source {}", "中文🙂 context ".repeat(800));
        let (path, input, page) = indexed(&fixture, source.as_bytes(), "source.txt", "source");
        assert!(page.next_offset.is_some());
        let (_, visual) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| Harness), window, cx));
        let handle = visual.update(|window, _| window.window_handle());
        visual.simulate_window_resize(handle, size(px(1000.), px(900.)));
        let viewer = visual
            .update(|window, cx| open(fixture.binding.clone(), input, page.clone(), window, cx));
        tap(visual, "knowledge-citation-next");
        wait(visual, |cx| {
            viewer.read(cx).page.offset > 0 && !viewer.read(cx).pending
        });
        assert_eq!(
            visual.update(|_, cx| viewer.read(cx).binding.client.target()),
            fixture.node.id()
        );
        assert_eq!(
            visual.update(|_, cx| viewer.read(cx).page.citation.clone()),
            page.citation
        );
        tap(visual, "knowledge-citation-previous");
        wait(visual, |cx| {
            viewer.read(cx).page.offset == 0 && !viewer.read(cx).pending
        });
        assert_eq!(
            visual.update(|_, cx| viewer.read(cx).page.text.clone()),
            page.text
        );
        std::fs::write(path, "changed source").unwrap();
        tap(visual, "knowledge-citation-next");
        wait(visual, |cx| !viewer.read(cx).pending);
        assert_eq!(
            visual.update(|_, cx| viewer.read(cx).page.text.clone()),
            page.text
        );
        assert_eq!(visual.update(|_, cx| viewer.read(cx).page.offset), 0);
        assert!(visual.update(|window, cx| {
            crate::feedback::tests::summary(window, cx)
                .contains(tr("knowledge_citation_changed").as_ref())
        }));
        tap(visual, "knowledge-citation-close");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn shows_typed_office_context_on_each_path(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let (_path, input, page) = indexed(
            &fixture,
            include_bytes!("../../../../../../crates/node-runtime/src/office/fixtures/report.pdf"),
            "report.pdf",
            "Second page",
        );
        assert_eq!(page.citation.position, Position::Page { page: 2 });
        assert!(page.text.contains("Second page"));
        let (_, visual) =
            cx.add_window_view(|window, cx| Root::new(cx.new(|_| Harness), window, cx));
        let viewer = visual
            .update(|window, cx| open(fixture.binding.clone(), input, page.clone(), window, cx));
        draw(visual);
        assert!(visual.debug_bounds("knowledge-citation-context").is_some());
        assert_eq!(
            visual.update(|_, cx| viewer.read(cx).page.citation.position.clone()),
            Position::Page { page: 2 }
        );
        assert_eq!(
            visual.update(|_, cx| viewer.read(cx).page.text.clone()),
            page.text
        );
        tap(visual, "knowledge-citation-close");
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[test]
fn presents_untrusted_context_as_literal_text() {
    let text = "```\n[private](file:///private)\n![image](https://invalid.example/pixel)\n```";
    let displayed = literal(text);
    assert!(displayed.starts_with("````\n"));
    assert!(displayed.contains(text));
    assert!(displayed.ends_with("\n````"));
    let parsed = crate::content::markdown::with_ranges(&displayed);
    assert_eq!(parsed.doc.blocks.len(), 1);
    assert!(
        matches!(&parsed.doc.blocks[0].kind, crate::content::markdown::BlockKind::Code { code, .. } if code.text == text && code.marks.is_empty())
    );
}
