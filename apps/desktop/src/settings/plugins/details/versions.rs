use super::*;
use gpui_kit::component::tag::Tag;

fn note_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.strip_prefix("- ").unwrap_or(line))
}

impl Details {
    pub(super) fn versions(&self, info: &Info, cx: &App) -> AnyElement {
        let installed = matches!(self.target, Target::Installed(_));
        let candidate = installed
            .then(|| self.updates.read(cx).candidate(&info.summary))
            .flatten();
        let mut rows = Vec::new();
        if let Some(candidate) = candidate {
            rows.push(
                super::rows::row(
                    "plugin-latest-version".into(),
                    IconName::Tag,
                    candidate
                        .summary
                        .version
                        .clone()
                        .unwrap_or_else(|| "—".into()),
                    None,
                    Tag::primary().small().child(tr("market_details_latest")),
                    cx,
                )
                .into_any_element(),
            );
            if let Some(notes) = candidate.release_notes.as_ref() {
                rows.push(
                    v_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .text_sm()
                        .children(
                            note_lines(notes.text(&rust_i18n::locale()))
                                .map(|line| div().child(line.to_owned())),
                        )
                        .into_any_element(),
                );
            }
            let added = self.updates.read(cx).added_actions(info);
            let labels: Vec<_> = candidate
                .extension
                .iter()
                .flat_map(|extension| &extension.actions)
                .filter(|action| added.contains(&permissions::raw(action)))
                .map(|action| tr(permissions::label(action)).to_string())
                .collect();
            if !labels.is_empty() {
                rows.push(
                    h_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .items_start()
                        .text_sm()
                        .text_color(cx.theme().warning)
                        .child(Icon::new(IconName::Shield).size_4().flex_shrink_0())
                        .child(format!(
                            "{}: {}",
                            tr("market_details_added_permissions"),
                            labels.join(", ")
                        ))
                        .into_any_element(),
                );
            }
        }
        if let Some(blocked) = installed
            .then(|| self.updates.read(cx).blocked(&info.summary))
            .flatten()
        {
            rows.push(
                super::rows::row(
                    "plugin-blocked-version".into(),
                    IconName::Tag,
                    blocked.version.clone(),
                    Some(compatibility::message(&blocked.required, &self.binding, cx).to_string()),
                    h_flex().children(compatibility::action(
                        "plugin-version-update-sailry",
                        &self.binding,
                        cx,
                    )),
                    cx,
                )
                .into_any_element(),
            );
        }
        rows.push(
            super::rows::row(
                "plugin-installed-version".into(),
                IconName::Tag,
                self.version().unwrap_or_else(|| "—".into()),
                None,
                div().when(installed, |view| {
                    view.child(super::rows::tag(tr("market_installed")))
                }),
                cx,
            )
            .into_any_element(),
        );
        if !installed {
            let notes = info.release_notes.as_ref().or(match &self.target {
                Target::Catalog { entry, .. } => entry.release_notes.as_ref(),
                _ => None,
            });
            if let Some(notes) = notes {
                rows.push(
                    v_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .text_sm()
                        .children(
                            note_lines(notes.text(&rust_i18n::locale()))
                                .map(|line| div().child(line.to_owned())),
                        )
                        .into_any_element(),
                );
            }
        }
        super::rows::section("market_details_versions", None, rows, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn preserves_plain_release_notes() {
        assert_eq!(
            note_lines("- Improved tools\n\n<script>alert(1)</script>\nhttps://example.org")
                .collect::<Vec<_>>(),
            [
                "Improved tools",
                "<script>alert(1)</script>",
                "https://example.org"
            ]
        );
    }
}
