//! Content visibility shared by controllers without changing canonical history.
use sailry_protocol::conversation::{Draft, Page, Part, Run, Status};

pub fn visible(run: &Run, page: &Page, drafts: &[Draft]) -> bool {
    run.continuation.is_none()
        || run.status != Status::Continued
        || page
            .entries
            .iter()
            .filter(|entry| entry.turn == run.turn && entry.author != "user")
            .flat_map(|entry| &entry.parts)
            .chain(
                drafts
                    .iter()
                    .filter(|draft| draft.turn == run.turn)
                    .flat_map(|draft| &draft.parts),
            )
            .any(|part| match part {
                Part::Text(text) | Part::Thinking(text) => !text.trim().is_empty(),
                Part::Resource(_) => false,
                _ => true,
            })
}
