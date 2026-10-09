use super::*;
use core::prelude::v1::test;
use sailry_protocol::{KnowledgeId, knowledge::Citation};
use std::time::{Duration, Instant};

fn wait(cx: &mut VisualTestContext, ready: impl Fn(&mut Window, &mut App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
            ready(window, cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "knowledge link UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui::test]
fn jumps_to_originating_worktree_line(cx: &mut TestAppContext) {
    use sailry_client::Client;
    use sailry_protocol::knowledge::{Definition, Mode, Query, Source};
    cx.executor().allow_parking();
    let fixture = crate::activity::fixture::Fixture::new();
    cx.update(|cx| {
        cx.set_global(fixture.services());
    });
    rust_i18n::set_locale("en");
    let (shell, mut visual) = crate::shell::tests::setup(cx);
    let mut previous_link = None;
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        let client = Client::new(fixture.nodes[index].local());
        let execute = |command| {
            fixture
                .runtime
                .block_on(client.execute(client.prepare(command)))
                .unwrap()
        };
        let Output::Snapshot(snapshot) = execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let tree = snapshot
            .worktrees
            .iter()
            .find(|tree| tree.id == fixture.sessions[index].worktree)
            .unwrap();
        let source = format!(
            "{}citationtarget on host {index}\n",
            (0..100)
                .map(|number| format!("padding line {number}\n"))
                .collect::<String>()
        );
        std::fs::write(std::path::Path::new(&tree.path).join("jump.txt"), &source).unwrap();
        let Output::KnowledgeSettings(mut settings) = execute(Command::ReadKnowledgeSettings)
        else {
            panic!("settings expected")
        };
        settings.model = None;
        execute(Command::SaveKnowledgeSettings(settings));
        let Output::KnowledgeCollection(collection) = execute(Command::CreateKnowledgeCollection {
            definition: Definition {
                name: "Jump source".into(),
                source: Source::Worktree {
                    worktree: tree.id,
                    path: "jump.txt".into(),
                },
                model: None,
                globs: Vec::new(),
                tags: Vec::new(),
                shared_with: Vec::new(),
            },
            plugin: None,
        }) else {
            panic!("collection expected")
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        let citation = loop {
            let Output::KnowledgeSearch(page) = execute(Command::QueryKnowledge(Query {
                query: "citationtarget".into(),
                collections: vec![collection.id],
                mode: Mode::Keyword,
                tags: Vec::new(),
                path: None,
                modified_after_ms: None,
                modified_before_ms: None,
                limit: 5,
                offset: 0,
            })) else {
                panic!("search expected")
            };
            if let Some(hit) = page.hits.into_iter().next() {
                break hit.citation;
            }
            assert!(Instant::now() < deadline, "knowledge index deadline");
            std::thread::sleep(Duration::from_millis(10));
        };
        let Position::Lines { start, .. } = citation.position else {
            panic!("line citation expected")
        };
        assert!(start > 1);
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(
                    &crate::live::menus::Dispatch {
                        node,
                        target: crate::live::menus::Target::Host,
                        command: crate::live::menus::Command::Open,
                    },
                    window,
                    cx,
                )
            })
        });
        wait(&mut visual, |_, cx| {
            shell.read(cx).live.as_ref().is_some_and(|live| {
                live.selected == node
                    && live
                        .view
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.node == node)
            })
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(fixture.sessions[index].clone(), window, cx)
            })
        });
        wait(&mut visual, |_, cx| {
            shell
                .read(cx)
                .current_chat()
                .is_some_and(|chat| chat.read(cx).binding().client.target() == node)
        });
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let chat = shell.current_chat().unwrap().clone();
                shell.open_chat_link(&chat, citation.link.clone().into(), window, cx);
            })
        });
        wait(&mut visual, |_, cx| {
            matches!(&shell.read(cx).side_resource, Some(crate::resources::SideResource::Plugin(panel))
            if panel.read(cx).document_scope(cx) == Some((node, tree.id)) && panel.read(cx).documents.as_ref().and_then(|documents| documents.read(cx).editor("jump.txt")).is_some_and(|editor| editor.read(cx).cursor_position().line == start - 1))
        });
        let text = visual.update(|_, cx| {
            let Some(crate::resources::SideResource::Plugin(panel)) = &shell.read(cx).side_resource
            else {
                panic!("document panel expected")
            };
            panel
                .read(cx)
                .documents
                .as_ref()
                .unwrap()
                .read(cx)
                .editor("jump.txt")
                .unwrap()
                .read(cx)
                .value()
                .to_string()
        });
        assert_eq!(text, source);
        if let Some(link) = previous_link.take() {
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    let chat = shell.current_chat().unwrap().clone();
                    shell.open_chat_link(&chat, link, window, cx);
                })
            });
            wait(&mut visual, |window, cx| {
                !window.notifications(cx).is_empty()
                    && crate::feedback::tests::summary(window, cx)
                        .contains(tr("knowledge_denied").as_ref())
            });
            assert!(visual.update(|_, cx| matches!(&shell.read(cx).side_resource, Some(crate::resources::SideResource::Plugin(panel)) if panel.read(cx).document_scope(cx) == Some((node, tree.id)))));
        }
        previous_link = Some(citation.link.into());
    }
    visual.update(|window, _| window.remove_window());
    fixture.close();
}

fn citation() -> Citation {
    let mut citation = Citation {
        collection: KnowledgeId::new(),
        chunk: "1".repeat(64),
        revision: "2".repeat(64),
        path: "private/source.xlsx".into(),
        position: Position::Cells {
            sheet: "Sheet".into(),
            start: "A1".into(),
            end: "B4".into(),
        },
        link: String::new(),
    };
    citation.link = citation.url();
    citation
}

#[test]
fn parses_only_machine_source_identity() {
    let citation = citation();
    let parsed = parse(&citation.link).unwrap();
    assert_eq!(parsed.collection, citation.collection);
    assert_eq!(parsed.chunk, citation.chunk);
    assert_eq!(parsed.revision, citation.revision);
    assert_eq!(parsed.offset, 0);
    for link in [
        format!("{}&path=../../private", citation.link),
        format!("{}#L9", citation.link),
        citation.link.replace("?revision=", "/extra?revision="),
        citation
            .link
            .replace("?revision=", "?revision=bad&revision="),
        citation.link.replace("sailry-knowledge://", "https://"),
        citation
            .link
            .replace("sailry-knowledge://", "sailry-knowledge://user@"),
    ] {
        assert!(parse(&link).is_err(), "accepted {link}");
    }
}

#[test]
fn rejects_mismatched_page_identity() {
    let citation = citation();
    let input = parse(&citation.link).unwrap();
    let mut page = CitationPage {
        citation,
        title: "Source".into(),
        text: "text".into(),
        offset: 0,
        next_offset: None,
        worktree: None,
    };
    assert!(matches(&page, &input));
    page.citation.collection = KnowledgeId::new();
    assert!(!matches(&page, &input));
    page.citation.collection = input.collection;
    page.citation.link = "file:///private/source.xlsx".into();
    assert!(!matches(&page, &input));
}
