//! UI-independent ordered page validation and directory refresh.
use sailry_protocol::{
    Command, Directory, ErrorCode, Fault, MAX_DIRECTORY_ENTRIES, Output, WorktreeId,
};

use crate::Client;

impl Client {
    /// Read the first page, or append one continuation without changing the old view on failure.
    pub async fn list_directory(
        &self,
        worktree: WorktreeId,
        path: &str,
        previous: Option<&Directory>,
    ) -> Result<Directory, Fault> {
        let after = match previous {
            Some(previous) if previous.path == path => {
                Some(previous.next.clone().ok_or_else(invalid)?)
            }
            Some(_) => return Err(invalid()),
            None => None,
        };
        let output = self
            .execute(self.prepare(Command::ListDirectory {
                worktree,
                path: path.into(),
                after,
            }))
            .await?;
        let Output::Directory(page) = output else {
            return Err(invalid());
        };
        append(path, previous, page)
    }

    /// Refresh the visible extent from the first page. Never combine old and new revisions.
    pub async fn refresh_directory(
        &self,
        worktree: WorktreeId,
        path: &str,
        visible: usize,
    ) -> Result<Directory, Fault> {
        let mut directory = self.list_directory(worktree, path, None).await?;
        while directory.entries.len() < visible && directory.next.is_some() {
            directory = self
                .list_directory(worktree, path, Some(&directory))
                .await?;
        }
        Ok(directory)
    }
}

fn append(
    path: &str,
    previous: Option<&Directory>,
    mut page: Directory,
) -> Result<Directory, Fault> {
    if page.path != path
        || page.entries.len() > MAX_DIRECTORY_ENTRIES
        || page
            .entries
            .windows(2)
            .any(|pair| pair[0].sort_key() >= pair[1].sort_key())
        || page.revision.is_empty()
        || page.truncated != page.next.is_some()
    {
        return Err(invalid());
    }
    let names: std::collections::BTreeSet<_> =
        page.entries.iter().map(|entry| &entry.name).collect();
    if names.len() != page.entries.len() {
        return Err(invalid());
    }
    if let Some(next) = &page.next
        && (!page.truncated
            || page.entries.len() != MAX_DIRECTORY_ENTRIES
            || next.revision != page.revision
            || page
                .entries
                .last()
                .is_none_or(|entry| entry.sort_key() != next.sort_key()))
    {
        return Err(invalid());
    }
    let Some(previous) = previous else {
        return Ok(page);
    };
    let cursor = previous.next.as_ref().ok_or_else(invalid)?;
    if previous.revision != page.revision || cursor.revision != page.revision {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "directory changed; restart listing",
        ));
    }
    if page
        .entries
        .first()
        .is_some_and(|entry| entry.sort_key() <= cursor.sort_key())
        || previous
            .entries
            .iter()
            .any(|entry| names.contains(&entry.name))
    {
        return Err(invalid());
    }
    let mut entries = previous.entries.clone();
    entries.append(&mut page.entries);
    page.entries = entries;
    Ok(page)
}

fn invalid() -> Fault {
    Fault::new(ErrorCode::InvalidRequest, "invalid directory page")
}

#[cfg(test)]
mod tests;
