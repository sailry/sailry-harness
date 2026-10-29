//! Captured, portable data recipes reuse the ordinary conversation surfaces.
use super::*;
use sailry_protocol::tool::projection::Format;
use serde_json::Value;

pub(super) struct Group {
    pub count: usize,
    pub content: AnyElement,
}

pub(super) fn grouped(call: &Call, page: &Page) -> bool {
    call.details(page)
        && call
            .resolved()
            .and_then(|resolved| resolved.output.as_ref())
            .is_some_and(|output| match output.paths.as_slice() {
                [] => false,
                [path] => summary(call, page).detail.text.as_ref() != path.as_str(),
                _ => true,
            })
}

fn paths(calls: &[&Call]) -> Vec<SharedString> {
    let mut paths = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for call in calls {
        if call.detail.is_none() {
            continue;
        }
        let Some(output) = call
            .resolved()
            .and_then(|resolved| resolved.output.as_ref())
        else {
            continue;
        };
        for path in &output.paths {
            if seen.insert(path.to_owned()) {
                paths.push(SharedString::from(path.to_owned()));
            }
        }
    }
    paths
}

pub(super) fn group(
    view: &View,
    calls: &[&Call],
    page: &Page,
    cx: &mut Context<View>,
) -> Option<Group> {
    let first = *calls.first()?;
    if !grouped(first, page) {
        return None;
    }
    let paths = paths(calls);
    let declared = calls
        .iter()
        .filter_map(|call| {
            call.resolved()?
                .output
                .as_ref()
                .map(|output| output.path_count)
        })
        .max()
        .unwrap_or_default();
    let failures: Vec<_> = calls
        .iter()
        .filter_map(|call| {
            let result = call.result(page)?;
            call.diagnostics(page).then_some((*call, result))
        })
        .collect();
    Some(Group {
        count: paths.len().max(declared).max(failures.len()),
        content: v_flex()
            .gap_2()
            .children(paths.into_iter().enumerate().map(|(index, path)| {
                div()
                    .w_full()
                    .font_normal()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(disclosure::summary(
                        format!(
                            "live-tool-file-{}-{}-{index}",
                            first.turn,
                            first.source.key()
                        ),
                        IconName::FileText,
                        path,
                        Detail {
                            secondary: true,
                            hide_caret: true,
                            ..Default::default()
                        },
                        cx,
                    ))
            }))
            .children(failures.into_iter().map(|(call, result)| {
                view.generic(
                    &format!("{}-{}", call.turn, call.source.key()),
                    call,
                    result,
                    cx,
                )
            }))
            .into_any_element(),
    })
}

pub(super) fn render(
    view: &View,
    key: &str,
    call: &Call,
    page: &Page,
    result: &Value,
    cx: &mut Context<View>,
) -> Option<AnyElement> {
    call.resolved()?.output.as_ref()?;
    let output = call.output(page)?;
    if let Some(data) = &output.table {
        let content = sailry_protocol::tool::Content {
            version: 1,
            blocks: vec![sailry_protocol::tool::Block::Table(data.clone())],
        };
        return Some(view.structured(key, call, page, &content, result, cx));
    }
    if let Some(body) = output
        .body
        .as_ref()
        .filter(|body| !body.text.is_empty() || body.empty.is_some())
    {
        let text = body.text.as_str();
        let path = body.path.as_deref();
        let mut actions = Vec::new();
        if !text.is_empty() {
            actions.push(view.copy_result(key, call, page, cx));
        }
        if let Some(style) = body.diff {
            return Some(
                diff::file_rows(
                    key,
                    text,
                    path.unwrap_or_default(),
                    style == Format::Added,
                    actions,
                )
                .into_any_element(),
            );
        }
        let content =
            if text.is_empty() {
                v_flex()
                    .children(body.empty.as_ref().map(|empty| {
                        surface::notice(empty.label(&rust_i18n::locale()).to_owned(), cx)
                    }))
                    .into_any_element()
            } else {
                view.code(&format!("{key}-output"), call, text, cx)
            };
        return Some(surface::result(key, content, actions, cx).into_any_element());
    }
    if output.preview.is_empty() && output.notices.is_empty() {
        return None;
    }
    let text = &output.preview;
    let notices: Vec<_> = output
        .notices
        .iter()
        .map(|notice| notice.label(&rust_i18n::locale()).to_owned())
        .collect();
    let copy = if text.is_empty() {
        notices.join("\n")
    } else {
        text.clone()
    };
    let content = v_flex()
        .when(!text.is_empty(), |column| {
            column.child(view.code(&format!("{key}-output"), call, text, cx))
        })
        .children(
            notices
                .into_iter()
                .map(|notice| surface::notice(notice, cx)),
        );
    let mut actions = Vec::new();
    if !copy.is_empty() {
        actions.push(view.copy_result(key, call, page, cx));
    }
    Some(surface::result(key, content.into_any_element(), actions, cx).into_any_element())
}
