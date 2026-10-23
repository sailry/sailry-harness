use super::super::inline::{active, active_content, content, icon, links, marker, token};
use super::*;

#[test]
fn matches_whole_labels_and_excludes_code() {
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
        ["@文档 notes.md (2)", "@文档 notes.md", "@Host"]
    );
    assert_eq!(active(text, &references), references);
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
            "unexpected reference: {text}"
        );
    }
}

#[test]
fn picker_skills_keep_a_distinct_icon() {
    for name in ["analysis", "pdf", "powerpoint", "word", "excel"] {
        let reference = Reference {
            target: Target::Skill {
                package: "reports".into(),
                name: name.into(),
            },
            label: name.into(),
        };
        assert_eq!(icon(&reference), "reicon:school/book");
    }
    assert_ne!(
        icon(&Reference {
            target: Target::Plugin("reports".into()),
            label: "Reports".into(),
        }),
        "reicon:school/book"
    );
}

#[test]
fn markers_keep_text_and_targets_without_icons() {
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
        assert_eq!(matched.len(), 1);
        assert_eq!(&text[matched[0].range.clone()], "@Selected content");
        assert_eq!(matched[0].id, text);
        assert_eq!(active(&text, std::slice::from_ref(&reference)), [reference]);
    }
}

#[test]
fn native_identity_rejects_plain_text() {
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
    assert!(active_content(&plain, &catalog).is_empty());
    let native = plain.with_token(0..5, token(&skill)).unwrap();
    assert_eq!(
        active_content(&native, &catalog).as_slice(),
        std::slice::from_ref(&skill)
    );
    assert_ne!(token(&file).id(), token(&skill).id());
    let mut renamed = skill;
    renamed.label = "Renamed".into();
    assert_eq!(token(&renamed).id(), native.tokens()[0].token().id());
    assert_eq!(content("@Same", &[file]).unwrap().text(), "@Same");
}
