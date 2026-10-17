use super::*;
use crate::{
    settings::{entry::Entry, group::Group},
    theme::DialogStyle as _,
};
use sailry_protocol::plugin::Skill;

pub(super) fn open(info: Info, skill: Skill, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let source = info.skill.as_ref().expect("standalone skill provenance");
        let mut information = Group::new("plugins_information").outline();
        for (key, value) in [
            ("plugins_identifier", Some(skill.name.clone())),
            (
                if source.git().is_some() {
                    "skills_repository"
                } else {
                    "skill_source_origin"
                },
                Some(provenance(&info)),
            ),
            (
                "skills_revision",
                source.git().map(|source| source.source.git_ref.clone()),
            ),
            (
                "skills_path",
                match source {
                    sailry_protocol::plugin::skills::Provenance::Imported { imported } => {
                        Some(imported.path.clone())
                    }
                    _ => source.git().map(|source| source.path.clone()),
                },
            ),
            (
                "plugins_version",
                match source {
                    sailry_protocol::plugin::skills::Provenance::Registry { registry } => {
                        Some(registry.version.clone())
                    }
                    _ => None,
                },
            ),
            ("skills_license", skill.license.clone()),
            ("skills_compatibility", skill.compatibility.clone()),
        ] {
            if let Some(value) = value {
                information = information.child(Entry::new(
                    format!("skill-info-{key}"),
                    v_flex().gap_1().child(tr(key)).child(
                        div()
                            .whitespace_normal()
                            .text_color(cx.theme().muted_foreground)
                            .child(value),
                    ),
                ));
            }
        }
        dialog
            .form_title(
                skill
                    .display_name
                    .clone()
                    .unwrap_or_else(|| skill.name.clone()),
                cx,
            )
            .w((window.viewport_size().width - px(48.)).min(px(520.)))
            .max_h(window.viewport_size().height * 0.8)
            .child(
                v_flex()
                    .gap_4()
                    .debug_selector(|| "skill-details".into())
                    .child(crate::plugins::emblem::render(
                        &info.summary.name,
                        Some(&sailry_protocol::plugin::desktop::Icon::Name(
                            "reicon:school/book".into(),
                        )),
                        None,
                        px(48.),
                        cx,
                    ))
                    .child(
                        div()
                            .whitespace_normal()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(skill.description.clone()),
                    )
                    .child(information),
            )
    });
}
