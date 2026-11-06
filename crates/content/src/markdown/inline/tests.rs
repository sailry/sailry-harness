// Adapted from Bezel 4a7505ab, crates/markdown (MIT). See third_party_licenses/bezel.md.
use super::*;
use gpui_kit as gpui;
use gpui_kit::component::{ActiveTheme, ThemeMode};

#[gpui::test]
fn underlines_only_the_hovered_link(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        let text = Text {
            text: "first second".into(),
            marks: vec![
                super::super::doc::MarkSpan {
                    range: 0..5,
                    mark: super::super::doc::Mark::Link("https://example.com/one".into()),
                },
                super::super::doc::MarkSpan {
                    range: 6..12,
                    mark: super::super::doc::Mark::Link("https://example.com/two".into()),
                },
            ],
        };
        let theme = Theme::of(cx);
        for hovered in [None, Some(2), Some(8), None] {
            let mut flat = flatten(&text, FontWeight::NORMAL, &theme);
            let original = flatten(&text, FontWeight::NORMAL, &theme);
            let mut undecorated = flatten(&text, FontWeight::NORMAL, &theme);
            undecorated.hover_link(None);
            assert_eq!(undecorated.links, original.links);
            assert_eq!(undecorated.text, original.text);
            for (run, original) in undecorated.runs.iter().zip(&original.runs) {
                assert!(run.underline.is_none());
                assert_eq!(run.color, original.color);
                assert_eq!(run.len, original.len);
            }
            for index in 0..=text.text.len() {
                assert_eq!(
                    undecorated
                        .offsets
                        .source(undecorated.offsets.display(index)),
                    index
                );
            }
            flat.hover_link(hovered.map(|index| flat.offsets.display(index)));
            let mut start = 0;
            for run in &flat.runs {
                let end = start + run.len;
                let source = flat.offsets.source(start)..flat.offsets.source(end);
                let expected = source.start < source.end
                    && hovered.is_some_and(|index| {
                        (index < 5 && source.start < 5) || (index >= 6 && source.start >= 6)
                    });
                assert_eq!(run.underline.is_some(), expected);
                start = end;
            }
            assert_eq!(
                flat.text.as_ref(),
                "\u{2001}\u{2060}first \u{2001}\u{2060}second"
            );
            assert_eq!(flat.links.len(), 2);
            for index in 0..=text.text.len() {
                assert_eq!(flat.offsets.source(flat.offsets.display(index)), index);
            }
            assert_eq!(text.text, "first second");
        }
    });
}

#[gpui::test]
fn reference_annotations_preserve_text_and_offsets(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        let text = Text::plain("中 @Host tail");
        let annotation = super::super::render::Annotation {
            at: super::super::Cursor::new(0, super::super::Part::Body, 0),
            range: 4..9,
            icon: Some("reicon:devices/computer".into()),
        };
        let theme = Theme::of(cx);
        let flat = flatten_with(&text, FontWeight::NORMAL, &theme, &[annotation]);
        assert_eq!(flat.text.as_ref(), text.text);
        assert_eq!(flat.references, [(4..5, "reicon:devices/computer".into())]);
        assert_eq!(
            flat.runs.iter().map(|run| run.len).sum::<usize>(),
            text.text.len()
        );
        assert!(
            flat.runs
                .iter()
                .any(|run| run.len == 1 && run.color.a == 0.)
        );
        assert!(flat.runs.iter().any(|run| run.color == theme.link));
        for at in text
            .text
            .char_indices()
            .map(|(at, _)| at)
            .chain([text.text.len()])
        {
            assert_eq!(flat.offsets.source(flat.offsets.display(at)), at);
        }
        assert!(text.marks.is_empty());
    });
}

#[gpui::test]
fn decorations_preserve_offsets_and_follow_theme(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::tests::init(cx);
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            gpui_kit::component::Theme::change(mode, None, cx);
            let link = gpui::rgb(if mode == ThemeMode::Dark {
                0x8DBEFF
            } else {
                0x2364C2
            })
            .into();
            gpui_kit::component::Theme::update(cx, |theme| theme.link = link);
            let theme = Theme::of(cx);
            let doc = super::super::parse::parse(
                "前`read_file`，[**中文**](notes.md)及[`code`](file.rs)结束",
            );
            let text = doc.blocks[0].text_at(super::super::Part::Body).unwrap();
            let flat = flatten(text, FontWeight::NORMAL, &theme);
            assert_eq!(flat.icons.len(), 2);
            assert_eq!(flat.code.len(), 2);
            assert_eq!(
                flat.runs.iter().map(|run| run.len).sum::<usize>(),
                flat.text.len()
            );
            for at in text
                .text
                .char_indices()
                .map(|(at, _)| at)
                .chain([text.text.len()])
            {
                assert_eq!(flat.offsets.source(flat.offsets.display(at)), at);
            }
            for range in &flat.code {
                assert!(flat.text[range.clone()].starts_with('\u{202f}'));
                assert!(flat.text[range.clone()].ends_with('\u{202f}'));
            }
            for icon in &flat.icons {
                assert_eq!(
                    flat.offsets.source(icon.start),
                    flat.offsets.source(icon.end)
                );
            }
            assert_eq!(theme.link, cx.theme().link);
            assert_eq!(theme.link, link);
            assert_ne!(theme.link, theme.text);
            assert!(flat.runs.iter().any(|run| run.color == theme.link));
            if mode == ThemeMode::Dark {
                assert!(theme.link.l > 0.6);
            }
        }
    });
}

mod insets {
    use super::*;
    use crate::markdown::parse::parse;

    fn font_at(flat: &Flat, at: usize) -> &gpui_kit::Font {
        let mut end = 0;
        flat.runs
            .iter()
            .find(|run| {
                end += run.len;
                at < end
            })
            .map(|run| &run.font)
            .unwrap()
    }

    #[gpui::test]
    fn match_on_both_sides(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::tests::init(cx);
            let mut theme = Theme::of(cx);
            theme.font_body = "Test Body".into();
            theme.font_mono = "Test Mono".into();
            for source in [
                "before `code` after",
                "`code` after",
                "before `code`",
                "**before** `code` **after**",
                "[`code`](file.rs) after",
                "before [`code`](file.rs)",
                "中文`代码`中文",
            ] {
                let doc = parse(source);
                let text = doc.blocks[0].text_at(crate::markdown::Part::Body).unwrap();
                let flat = flatten(text, FontWeight::NORMAL, &theme);
                let inset = '\u{202f}'.len_utf8();
                let [range] = flat.code.as_slice() else {
                    panic!("one code span: {source}");
                };
                assert!(flat.text[range.clone()].starts_with('\u{202f}'), "{source}");
                assert!(flat.text[range.clone()].ends_with('\u{202f}'), "{source}");
                let leading = font_at(&flat, range.start);
                assert_eq!(leading, font_at(&flat, range.end - inset), "{source}");
                assert_eq!(leading, font_at(&flat, range.start + inset), "{source}");
                assert_eq!(leading.family.as_ref(), "Test Mono", "{source}");
            }
        });
    }

    #[gpui::test]
    fn keep_source_offsets(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::tests::init(cx);
            let theme = Theme::of(cx);
            let doc = parse("ab `cd` ef");
            let text = doc.blocks[0].text_at(crate::markdown::Part::Body).unwrap();
            assert_eq!(text.text, "ab cd ef");
            let flat = flatten(text, FontWeight::NORMAL, &theme);
            let inset = '\u{202f}'.len_utf8();
            assert_eq!(flat.text.as_ref(), "ab \u{202f}cd\u{202f} ef");
            assert_eq!(flat.code.len(), 1);
            assert_eq!(flat.code[0], 3..5 + 2 * inset);
            assert_eq!(
                flat.runs.iter().map(|run| run.len).sum::<usize>(),
                flat.text.len()
            );
            for (source, display) in [
                (0, 0),
                (3, 3 + inset),
                (4, 4 + inset),
                (5, 5 + 2 * inset),
                (8, 8 + 2 * inset),
            ] {
                assert_eq!(flat.offsets.display(source), display);
                assert_eq!(flat.offsets.source(display), source);
            }
            // A point inside an inset resolves to the source edge it pads.
            assert_eq!(flat.offsets.source(3 + inset - 1), 3);
            assert_eq!(flat.offsets.source(5 + 2 * inset - 1), 5);
        });
    }
}

mod link_gaps {
    use super::*;
    use crate::markdown::{Cursor, Doc, Mark, Part, Selection, parse::parse, selectable::copied};
    use gpui_kit::component::{Theme as Kit, ThemeColor};

    const GAP: &str = "\u{a0}";
    const ICON: &str = "\u{2001}\u{2060}";

    fn decorated(source: &str) -> (Doc, Flat) {
        let doc = parse(source);
        let theme = Theme::from_kit(&Kit::from(&*ThemeColor::light()));
        let text = doc.blocks[0].text_at(Part::Body).unwrap();
        let flat = flatten(text, FontWeight::NORMAL, &theme);
        (doc, flat)
    }

    #[test]
    fn separates_words_only() {
        for neighbour in ['给', '处', 'あ', '한', 'a', 'Z', '7', 'é', 'я', '🚀'] {
            assert!(needs_gap(Some(neighbour)), "{neighbour:?}");
        }
        for neighbour in [
            ' ', '\n', '\t', '\u{a0}', '\u{3000}', '(', ')', '[', ']', '"', '\'', ',', '.', ':',
            ';', '!', '?', '-', '/', '“', '”', '‘', '’', '«', '»', '…', '—', '（', '）', '「',
            '」', '《', '》', '【', '】', '，', '。', '、', '：', '；', '！', '？', '·',
        ] {
            assert!(!needs_gap(Some(neighbour)), "{neighbour:?}");
        }
        assert!(!needs_gap(None));
    }

    #[test]
    fn pad_only_word_neighbours() {
        for (source, display) in [
            (
                "交给[Reduce compaction cost](u)处理",
                format!("交给{GAP}{ICON}Reduce compaction cost{GAP}处理"),
            ),
            ("see[docs](u)now", format!("see{GAP}{ICON}docs{GAP}now")),
            ("see [docs](u) now", format!("see {ICON}docs now")),
            ("see [docs](u).", format!("see {ICON}docs.")),
            ("（[文档](u)）", format!("（{ICON}文档）")),
            ("参见：[文档](u)，然后", format!("参见：{ICON}文档，然后")),
            ("“[link](u)”", format!("“{ICON}link”")),
            ("[链接](u)结束", format!("{ICON}链接{GAP}结束")),
            ("开始[链接](u)", format!("开始{GAP}{ICON}链接")),
            ("前\n[链接](u)\n后", format!("前\n{ICON}链接\n后")),
            ("[a](u)[b](v)", format!("{ICON}a{GAP}{GAP}{ICON}b")),
            (
                "用`code`[链接](u)",
                format!("用\u{202f}code\u{202f}{GAP}{ICON}链接"),
            ),
            (
                "交给[`run`](u)处理",
                format!("交给{GAP}{ICON}\u{202f}run\u{202f}{GAP}处理"),
            ),
        ] {
            let (_, flat) = decorated(source);
            assert_eq!(&flat.text[..], display, "{source}");
        }
    }

    #[test]
    fn map_gaps_to_link_edges() {
        let theme = Theme::from_kit(&Kit::from(&*ThemeColor::light()));
        for source in [
            "交给[Reduce compaction cost](u)处理",
            "see[docs](u)now",
            "[a](u)[b](v)",
            "交给[`run`](u)处理",
            "前<https://example.com>后",
        ] {
            let (doc, mut flat) = decorated(source);
            let text = doc.blocks[0].text_at(Part::Body).unwrap();
            assert_eq!(
                flat.runs.iter().map(|run| run.len).sum::<usize>(),
                flat.text.len(),
                "{source}"
            );
            for at in text
                .text
                .char_indices()
                .map(|(at, _)| at)
                .chain([text.text.len()])
            {
                assert_eq!(
                    flat.offsets.source(flat.offsets.display(at)),
                    at,
                    "{source}"
                );
            }
            let edges: Vec<usize> = text
                .marks
                .iter()
                .filter(|span| matches!(span.mark, Mark::Link(_) | Mark::Mention { .. }))
                .flat_map(|span| [span.range.start, span.range.end])
                .collect();
            let mut start = 0;
            let mut gaps = 0;
            for run in &flat.runs {
                let range = start..start + run.len;
                start = range.end;
                if &flat.text[range.clone()] != GAP {
                    continue;
                }
                gaps += 1;
                let edge = flat.offsets.source(range.start);
                assert_eq!(flat.offsets.source(range.end), edge, "{source}");
                assert!(edges.contains(&edge), "{source}");
                assert!(
                    flat.links
                        .iter()
                        .all(|(link, _)| !link.contains(&range.start)),
                    "{source}"
                );
                assert!(run.underline.is_none(), "{source}");
                assert_eq!(run.color.a, 0., "{source}");
                assert_eq!(run.font.family, theme.font_body, "{source}");
            }
            assert_eq!(gaps, 2, "{source}");
            let label = flat.links[0].0.start + ICON.len();
            flat.hover_link(Some(label));
            let mut start = 0;
            for run in &flat.runs {
                let range = start..start + run.len;
                start = range.end;
                if run.underline.is_some() {
                    assert!(
                        flat.links[0].0.start <= range.start && range.end <= flat.links[0].0.end,
                        "{source}"
                    );
                }
            }
        }
    }

    #[test]
    fn copy_resolves_gaps_to_source() {
        let source = "交给[Reduce compaction cost](u)处理";
        let (doc, flat) = decorated(source);
        let gaps: Vec<usize> = flat.text.match_indices(GAP).map(|(at, _)| at).collect();
        let &[before, after] = gaps.as_slice() else {
            panic!("two gaps: {source}");
        };
        let from = Cursor::new(0, Part::Body, flat.offsets.source(before));
        let to = Cursor::new(0, Part::Body, flat.offsets.source(after + GAP.len()));
        assert_eq!(
            copied(&doc, Selection::new(from, to)),
            "Reduce compaction cost"
        );
        assert_eq!(
            copied(&doc, Selection::all(&doc)),
            "交给Reduce compaction cost处理"
        );
    }

    #[test]
    fn chip_wash_starts_at_icon() {
        let (_, flat) = decorated("前<https://example.com>后");
        assert_eq!(
            &flat.text[..],
            format!("前{GAP}{ICON}https://example.com{GAP}后")
        );
        let [(link, _)] = flat.links.as_slice() else {
            panic!("one link");
        };
        assert_eq!(flat.chips, [link.clone()]);
        assert_eq!(
            &flat.text[link.clone()],
            format!("{ICON}https://example.com")
        );
    }
}

mod emphasis {
    use super::*;
    use crate::markdown::{self, Part, Selection, doc::BlockKind};

    #[gpui::test]
    fn colors_only_prose(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::tests::init(cx);
            let cases = [
                (
                    "普通说明，**关键决定**，继续正文。",
                    "普通说明，关键决定，继续正文。",
                    Some("关键决定"),
                ),
                (
                    "Before **the action** and after.",
                    "Before the action and after.",
                    Some("the action"),
                ),
                (r"\*\*literal\*\*", "**literal**", None),
                ("**unfinished", "**unfinished", None),
                ("**`code`**", "code", None),
                ("`**code**`", "**code**", None),
                ("**prose `code`**", "prose code", Some("prose ")),
                ("[**link**](https://example.com)", "link", None),
                ("[**`code`**](https://example.com)", "code", None),
                ("# Heading", "Heading", None),
                ("```text\n**literal**\n```", "**literal**", None),
            ];
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                gpui_kit::component::Theme::change(mode, None, cx);
                let original = Theme::of(cx);
                let mut emphasized = original.clone();
                emphasized.strong_color = Some(cx.theme().magenta);
                assert_ne!(cx.theme().magenta, original.text);
                for (source, copied, target) in cases {
                    let parsed = markdown::with_ranges(source);
                    let block = &parsed.doc.blocks[0];
                    let text = block.text_at(block.parts()[0]).unwrap();
                    let weight = if matches!(&block.kind, BlockKind::Heading { .. }) {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    };
                    let before = flatten(text, weight, &original);
                    let after = flatten(text, weight, &emphasized);
                    assert_eq!(after.text, before.text, "{source}");
                    assert_eq!(after.code, before.code, "{source}");
                    assert_eq!(after.links, before.links, "{source}");
                    assert_eq!(after.icons, before.icons, "{source}");
                    assert_eq!(after.runs.len(), before.runs.len(), "{source}");
                    let target = target.map(|target| {
                        let start = after.text.find(target).unwrap();
                        start..start + target.len()
                    });
                    let mut start = 0;
                    for (before, after) in before.runs.iter().zip(&after.runs) {
                        let end = start + after.len;
                        assert_eq!(after.len, before.len, "{source}");
                        assert_eq!(after.font.weight, before.font.weight, "{source}");
                        assert_eq!(after.font.style, before.font.style, "{source}");
                        let expected = if target
                            .as_ref()
                            .is_some_and(|target| start >= target.start && end <= target.end)
                        {
                            cx.theme().magenta
                        } else {
                            before.color
                        };
                        assert_eq!(after.color, expected, "{source}");
                        start = end;
                    }
                    for at in text
                        .text
                        .char_indices()
                        .map(|(at, _)| at)
                        .chain([text.text.len()])
                    {
                        assert_eq!(
                            after.offsets.source(after.offsets.display(at)),
                            at,
                            "{source}"
                        );
                    }
                    assert_eq!(
                        markdown::selectable::copied(&parsed.doc, Selection::all(&parsed.doc)),
                        copied,
                        "{source}",
                    );
                    assert_eq!(
                        parsed
                            .block_ranges
                            .iter()
                            .map(|range| &source[range.clone()])
                            .collect::<String>(),
                        source,
                    );
                }
            }
        });
    }

    #[gpui::test]
    fn keeps_annotation_priority(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::tests::init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                gpui_kit::component::Theme::change(mode, None, cx);
                let original = Theme::of(cx);
                let mut emphasized = original.clone();
                emphasized.strong_color = Some(cx.theme().magenta);
                let doc = markdown::parse("**引用**");
                let text = doc.blocks[0].text_at(Part::Body).unwrap();
                for icon in [None, Some("reicon:devices/computer".into())] {
                    let annotations = [markdown::render::Annotation {
                        at: markdown::Cursor::new(0, Part::Body, 0),
                        range: 0..text.text.len(),
                        icon,
                    }];
                    let before = flatten_with(text, FontWeight::NORMAL, &original, &annotations);
                    let after = flatten_with(text, FontWeight::NORMAL, &emphasized, &annotations);
                    assert_eq!(after.text, before.text);
                    assert_eq!(after.references, before.references);
                    assert_eq!(after.runs.len(), before.runs.len());
                    for (before, after) in before.runs.iter().zip(&after.runs) {
                        assert_eq!(after.len, before.len);
                        assert_eq!(after.color, before.color);
                        assert_eq!(after.font.weight, before.font.weight);
                    }
                    assert_eq!(
                        markdown::selectable::copied(&doc, Selection::all(&doc)),
                        "引用",
                    );
                }
            }
        });
    }

    #[gpui::test]
    fn follows_complete_stream_source(cx: &mut gpui::TestAppContext) {
        use gpui_kit::AppContext as _;

        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::tests::init(cx);
            for mode in [ThemeMode::Light, ThemeMode::Dark] {
                gpui_kit::component::Theme::change(mode, None, cx);
                let state = cx.new(|cx| markdown::State::new("", cx));
                let mut theme = Theme::of(cx);
                theme.strong_color = Some(cx.theme().magenta);
                let mut source = String::new();
                for chunk in ["说明，", "**", "重点", "**", "；`**代码**`"] {
                    source.push_str(chunk);
                    state.update(cx, |state, cx| state.set_source(source.clone(), cx));
                    let state = state.read(cx);
                    assert_eq!(state.source(), source);
                    let parsed = markdown::with_ranges(&source);
                    assert_eq!(state.document.as_ref(), &parsed.doc);
                    let text = state.document.blocks[0].text_at(Part::Body).unwrap();
                    let flat = flatten(text, FontWeight::NORMAL, &theme);
                    assert_eq!(
                        flat.runs.iter().any(|run| run.color == cx.theme().magenta),
                        source.contains("**重点**"),
                    );
                    assert_eq!(
                        markdown::selectable::copied(
                            &state.document,
                            Selection::all(&state.document)
                        ),
                        markdown::selectable::copied(&parsed.doc, Selection::all(&parsed.doc)),
                    );
                }
                let state = state.read(cx);
                assert_eq!(state.source(), "说明，**重点**；`**代码**`");
                assert_eq!(
                    markdown::selectable::copied(&state.document, Selection::all(&state.document)),
                    "说明，重点；**代码**",
                );
            }
        });
    }
}
