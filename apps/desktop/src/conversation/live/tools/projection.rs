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
            .resolved
            .as_ref()
            .and_then(|resolved| resolved.output.as_ref())
            .is_some_and(|output| !output.paths.is_empty())
}

fn paths(calls: &[&Call]) -> Vec<SharedString> {
    let mut paths = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for call in calls {
        let Some(output) = call
            .resolved
            .as_ref()
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
    let failures: Vec<_> = calls
        .iter()
        .filter_map(|call| {
            let result = call.result(page)?;
            call.diagnostics(page).then_some((*call, result))
        })
        .collect();
    let worktree = view.turn_worktree(first.turn);
    Some(Group {
        count: paths.len().max(failures.len()),
        content: v_flex()
            .gap_2()
            .children(paths.into_iter().enumerate().map(|(index, path)| {
                disclosure::trigger(
                    format!(
                        "live-tool-file-{}-{}-{index}",
                        first.turn,
                        first.source.key()
                    ),
                    IconName::FileText,
                    path.clone(),
                    Detail {
                        secondary: true,
                        ..Default::default()
                    },
                    false,
                    cx,
                )
                .text_size(cx.theme().typography_tokens().sm.size)
                .font_normal()
                .font_family(cx.theme().mono_font_family.clone())
                .on_click(cx.listener(move |_, _, _, cx| {
                    if let Some(worktree) = worktree {
                        cx.emit(Event::FileAt(worktree, path.to_string()));
                    }
                }))
            }))
            .children(failures.into_iter().map(|(call, result)| {
                view.generic(
                    &format!("{}-{}", call.turn, call.source.key()),
                    call,
                    None,
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
    call.resolved.as_ref()?.output.as_ref()?;
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
            actions.push(view.copy_result(key, text.to_owned()));
        }
        if let Some(path) = path {
            actions.push(view.path_link(key, call.turn, path.to_owned().into(), cx));
        }
        if let Some(style) = body.diff {
            return Some(diff::file_rows(
                key,
                text,
                path.unwrap_or_default(),
                style == Format::Added,
                actions,
            ));
        }
        let content =
            if text.is_empty() {
                v_flex()
                    .children(body.empty.as_ref().map(|empty| {
                        surface::notice(empty.label(&rust_i18n::locale()).to_owned(), cx)
                    }))
                    .into_any_element()
            } else {
                surface::code(
                    &format!("{key}-output"),
                    text,
                    px(f32::from(crate::preferences::data(cx).message_text_size())),
                    cx,
                )
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
            column.child(surface::code(
                &format!("{key}-output"),
                text,
                px(f32::from(crate::preferences::data(cx).message_text_size())),
                cx,
            ))
        })
        .children(
            notices
                .into_iter()
                .map(|notice| surface::notice(notice, cx)),
        );
    let mut actions = Vec::new();
    if !copy.is_empty() {
        actions.push(view.copy_result(key, copy));
    }
    if let Some(path) = call
        .resolved
        .as_ref()
        .and_then(|resolved| resolved.input.target.as_ref())
        .map(|path| path.text.as_str())
        .filter(|path| !path.is_empty())
    {
        actions.push(view.path_link(key, call.turn, path.to_owned().into(), cx));
    }
    Some(surface::result(key, content.into_any_element(), actions, cx).into_any_element())
}
