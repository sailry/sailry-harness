//! Knowledge links keep the originating chat's Node and verified source identity.
use super::*;
use crate::{
    conversation::live::{Binding, View as Chat},
    theme::DialogStyle as _,
};
use sailry_protocol::{
    Command, ErrorCode, Fault, Output,
    knowledge::{CitationPage, CitationRequest, Position},
};

#[cfg(test)]
mod tests;
mod viewer;

pub(super) fn recognizes(link: &str) -> bool {
    link.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("sailry-knowledge"))
}

fn parse(link: &str) -> Result<CitationRequest, Fault> {
    let url =
        url::Url::parse(link).map_err(|_| Fault::invalid("invalid knowledge citation link"))?;
    if url.scheme() != "sailry-knowledge"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(Fault::invalid("invalid knowledge citation link"));
    }
    let collection = url
        .host_str()
        .ok_or_else(|| Fault::invalid("knowledge collection is missing"))?
        .parse()
        .map_err(|_| Fault::invalid("invalid knowledge collection"))?;
    let chunk = url
        .path()
        .strip_prefix('/')
        .ok_or_else(|| Fault::invalid("knowledge chunk is missing"))?
        .to_owned();
    let pairs = url.query_pairs().collect::<Vec<_>>();
    let [(key, revision)] = pairs.as_slice() else {
        return Err(Fault::invalid("knowledge revision is missing"));
    };
    if key != "revision" {
        return Err(Fault::invalid("invalid knowledge citation query"));
    }
    let input = CitationRequest {
        collection,
        chunk,
        revision: revision.to_string(),
        offset: 0,
        limit: 16 * 1024,
    };
    input.validate()?;
    Ok(input)
}

fn matches(page: &CitationPage, input: &CitationRequest) -> bool {
    page.citation.collection == input.collection
        && page.citation.chunk == input.chunk
        && page.citation.revision == input.revision
        && page.citation.link == page.citation.url()
        && page.offset == input.offset
        && page.text.len() <= input.limit as usize
        && page.next_offset.is_none_or(|next| {
            next == input.offset.saturating_add(page.text.len() as u32) && next > input.offset
        })
}

fn error_key(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::RevisionConflict | ErrorCode::NotFound => "knowledge_citation_changed",
        ErrorCode::PermissionDenied => "knowledge_denied",
        ErrorCode::InvalidRequest => "knowledge_citation_invalid",
        _ => "knowledge_citation_unavailable",
    }
}

pub(super) fn failure(key: &'static str, window: &mut Window, cx: &mut App) {
    crate::feedback::error(&tr("knowledge_source"), &tr(key), window, cx);
}

impl Shell {
    pub(super) fn open_knowledge_link(
        &mut self,
        chat: &Entity<Chat>,
        link: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = match parse(&link) {
            Ok(input) => input,
            Err(_) => {
                failure("knowledge_citation_invalid", window, cx);
                return;
            }
        };
        let binding = chat.read(cx).binding();
        let requested = input.clone();
        let client = binding.client.clone();
        let job = binding.runtime.spawn(async move {
            client
                .execute(client.prepare(Command::ReadKnowledgeCitation(requested)))
                .await
        });
        cx.spawn_in(window, async move |owner, cx| {
            let result = job.await;
            _ = owner.update_in(cx, |shell, window, cx| match result {
                Ok(Ok(Output::KnowledgeCitation(page))) if matches(&page, &input) => {
                    if let (Some(worktree), Position::Lines { start, .. }) =
                        (page.worktree, &page.citation.position)
                        && shell.current_chat().is_some_and(|chat| {
                            chat.read(cx).binding().client.target() == binding.client.target()
                        })
                    {
                        shell.open_conversation_file(
                            (binding.client.target(), worktree),
                            page.citation.path,
                            Some(*start as usize),
                            window,
                            cx,
                        );
                    } else {
                        viewer::open(binding, input, page, window, cx);
                    }
                }
                Ok(Err(error)) => failure(error_key(error.code), window, cx),
                _ => failure("knowledge_citation_unavailable", window, cx),
            });
        })
        .detach();
    }
}
