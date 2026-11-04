use super::super::inline::{active, active_content, content, icon, links, marker, token};
use super::*;

#[test]
fn markers_and_native_identity() {
    for (scenario, check) in [
        (
            "matches_whole_labels_and_excludes_code",
            (|_scenario: &str| {
                let references = vec![
                    Reference {
                        target: Target::File("a".into()),
                        label: "文档 notes.md".into(),
                    },
                    Reference {
                        target: Target::File("b".into()),
                        label: "文档 notes.md (2)".into(),
                    },
                    Reference {
                        target: Target::Host,
                        label: "Host".into(),
                    },
                ];
                let text = "看 @文档 notes.md (2)\n@文档 notes.md， @Host";
                let matched = links(text, &references);
                assert_eq!(
                    matched
                        .iter()
                        .map(|link| &text[link.range.clone()])
                        .collect::<Vec<_>>(),
                    ["@文档 notes.md (2)", "@文档 notes.md", "@Host"],
                    "matches whole labels and excludes code"
                );
                assert_eq!(
                    active(text, &references),
                    references,
                    "matches whole labels and excludes code"
                );
                for text in [
                    "`@Host`",
                    "```\n@Host\n```",
                    "[x](file:@Host)",
                    "<span title='@Host'></span>",
                    "x@Host",
                    "@Hostile",
                    "@文档 notes.m",
                ] {
                    assert!(
                        active(text, &references).is_empty(),
                        "matches whole labels and excludes code: unexpected reference: {text}"
                    );
                }
            }) as fn(&str),
        ),
        (
            "picker_skills_keep_a_distinct_icon",
            (|_scenario: &str| {
                for name in ["analysis", "pdf", "powerpoint", "word", "excel"] {
                    let reference = Reference {
                        target: Target::Skill {
                            package: "reports".into(),
                            name: name.into(),
                        },
                        label: name.into(),
                    };
                    assert_eq!(
                        icon(&reference),
                        "reicon:school/book",
                        "picker skills keep a distinct icon"
                    );
                }
                assert_ne!(
                    icon(&Reference {
                        target: Target::Plugin("reports".into()),
                        label: "Reports".into(),
                    }),
                    "reicon:school/book",
                    "picker skills keep a distinct icon"
                );
            }),
        ),
        (
            "markers_keep_text_and_targets_without_icons",
            (|_scenario: &str| {
                for target in [
                    Target::File("notes.md".into()),
                    Target::Directory("src".into()),
                    Target::Plugin("reports".into()),
                    Target::Skill {
                        package: "reports".into(),
                        name: "analysis".into(),
                    },
                    Target::Session(SessionId::new()),
                ] {
                    let reference = Reference {
                        target,
                        label: "Selected content".into(),
                    };
                    let text = marker(&reference);
                    let matched = links(&text, std::slice::from_ref(&reference));
                    assert_eq!(
                        matched.len(),
                        1,
                        "markers keep text and targets without icons"
                    );
                    assert_eq!(
                        &text[matched[0].range.clone()],
                        "@Selected content",
                        "markers keep text and targets without icons"
                    );
                    assert_eq!(
                        matched[0].id, text,
                        "markers keep text and targets without icons"
                    );
                    assert_eq!(
                        active(&text, std::slice::from_ref(&reference)),
                        [reference],
                        "markers keep text and targets without icons"
                    );
                }
            }),
        ),
        (
            "native_identity_rejects_plain_text",
            (|_scenario: &str| {
                use gpui_kit::base::input::InputContent;
                let file = Reference {
                    label: "Same".into(),
                    target: Target::File("notes.md".into()),
                };
                let skill = Reference {
                    label: "Same".into(),
                    target: Target::Skill {
                        package: "reports".into(),
                        name: "analysis".into(),
                    },
                };
                let catalog = vec![file.clone(), skill.clone()];
                let text = "@Same @Same";
                let plain = InputContent::new(text);
                assert!(
                    active_content(&plain, &catalog).is_empty(),
                    "native identity rejects plain text"
                );
                let native = plain.with_token(0..5, token(&skill)).unwrap();
                assert_eq!(
                    active_content(&native, &catalog).as_slice(),
                    std::slice::from_ref(&skill),
                    "native identity rejects plain text"
                );
                assert_ne!(
                    token(&file).id(),
                    token(&skill).id(),
                    "native identity rejects plain text"
                );
                let mut renamed = skill;
                renamed.label = "Renamed".into();
                assert_eq!(
                    token(&renamed).id(),
                    native.tokens()[0].token().id(),
                    "native identity rejects plain text"
                );
                assert_eq!(
                    content("@Same", &[file]).unwrap().text(),
                    "@Same",
                    "native identity rejects plain text"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
