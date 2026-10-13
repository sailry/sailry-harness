use super::*;

pub(super) fn visible(entries: &[FileEntry], hidden: bool, filter: &str) -> Vec<usize> {
    let filter = filter.to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            ((hidden || !entry.name.starts_with('.'))
                && entry.name.to_lowercase().contains(&filter))
            .then_some(index)
        })
        .collect()
}
pub(super) fn resolve(
    mode: &Mode,
    listing: &Listing,
    selected: &BTreeSet<usize>,
    filename: &str,
) -> Option<Selection> {
    let join = |name: &str| {
        format!(
            "{}{}{}",
            listing.directory.path.trim_end_matches(listing.separator),
            listing.separator,
            name
        )
    };
    match mode {
        Mode::Directory if selected.is_empty() => {
            Some(Selection::Directory(listing.directory.path.clone()))
        }
        Mode::Directory => {
            if selected.len() != 1 {
                return None;
            }
            let entry = listing.directory.entries.get(*selected.first()?)?;
            (entry.kind == EntryKind::Directory).then(|| Selection::Directory(join(&entry.name)))
        }
        Mode::Files { multiple } => {
            let files: Vec<_> = selected
                .iter()
                .filter_map(|index| listing.directory.entries.get(*index))
                .filter(|entry| entry.kind == EntryKind::File)
                .map(|entry| join(&entry.name))
                .collect();
            (!files.is_empty() && (*multiple || (selected.len() == 1 && files.len() == 1)))
                .then_some(Selection::Files(files))
        }
        Mode::Save { .. } => {
            let filename = filename.trim();
            (!filename.is_empty()
                && !matches!(filename, "." | "..")
                && !filename.contains(['/', '\\', '\0']))
            .then(|| Selection::Save(join(filename)))
        }
    }
}
