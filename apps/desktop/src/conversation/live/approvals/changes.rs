//! A change set awaits one decision; each Node-validated file shows its bounded diff.
use super::*;
use sailry_protocol::{ChangeKind, ChangePreview};

pub(super) fn preview(id: ApprovalId, changes: ChangePreview, cx: &App) -> AnyElement {
    let title = match changes.files.len() {
        1 => tr("file_changes_approval_one"),
        count => rust_i18n::t!("file_changes_approval", count = count)
            .to_string()
            .into(),
    };
    v_flex()
        .debug_selector(move || format!("live-approval-changes-{id}"))
        .w_full()
        .min_w_0()
        .gap_2()
        .child(div().whitespace_normal().child(title))
        .children(changes.files.into_iter().enumerate().map(|(index, file)| {
            let kind = tr(match file.kind {
                ChangeKind::Added => "file_changes_added",
                ChangeKind::Modified => "file_changes_modified",
                ChangeKind::Renamed => "file_changes_renamed",
                ChangeKind::Deleted => "file_changes_deleted",
            });
            let target = file
                .destination
                .clone()
                .unwrap_or_else(|| file.path.clone());
            let path = match &file.destination {
                Some(destination) => format!("{} → {destination}", file.path),
                None => file.path.clone(),
            };
            v_flex()
                .w_full()
                .min_w_0()
                .gap_1()
                .child(
                    h_flex()
                        .w_full()
                        .min_w_0()
                        .gap_2()
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(cx.theme().muted_foreground)
                                .child(kind),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(cx.theme().mono_font_family.clone())
                                .child(path),
                        )
                        .child(crate::conversation::surface::diff::badge(
                            file.additions,
                            file.deletions,
                            cx,
                        )),
                )
                .when(!file.diff.is_empty(), |column| {
                    column.child(crate::conversation::surface::diff::file_rows(
                        &format!("approval-changes-{id}-{index}"),
                        &file.diff,
                        &target,
                        false,
                        Vec::new(),
                    ))
                })
                .when(file.truncated, |column| {
                    column.child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(tr("file_changes_shortened")),
                    )
                })
        }))
        .into_any_element()
}
