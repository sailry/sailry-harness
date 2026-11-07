use super::*;

pub(in crate::git) fn inspect(
    root: &Path,
    environment: &crate::process::Environment,
    control: &Control,
) -> Result<GitStatus, Fault> {
    let repository = open(root)?;
    let before = metadata(root)?;
    let mut args = [
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
    ]
    .map(str::to_owned)
    .to_vec();
    let configured = Repository::open_ext(
        root,
        git2::RepositoryOpenFlags::NO_SEARCH,
        std::iter::empty::<&std::ffi::OsStr>(),
    )
    .map_err(fault)?;
    let config = configured.config().map_err(fault)?;
    let mut filters = config
        .entries(Some(r"^filter\..*\.(clean|process|required)$"))
        .map_err(fault)?;
    let mut drivers = BTreeSet::new();
    while let Some(entry) = filters.next() {
        control.check()?;
        let entry = entry.map_err(fault)?;
        if let Some(driver) = entry
            .name()
            .map_err(fault)?
            .strip_prefix("filter.")
            .and_then(|name| name.rsplit_once('.').map(|(driver, _)| driver))
        {
            drivers.insert(driver.to_owned());
        }
    }
    for driver in drivers {
        for (name, value) in [("clean", ""), ("process", ""), ("required", "false")] {
            args.extend(["-c".to_owned(), format!("filter.{driver}.{name}={value}")]);
        }
    }
    args.extend(
        [
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
            "--no-renames",
        ]
        .map(str::to_owned),
    );
    let capture = crate::git::cli::read(root, environment, &args, control)?;
    if capture.invalid_utf8 {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "Git status contains unsupported path encoding",
        ));
    }
    let mut result = parse(&capture.text, capture.truncated, root, control)?;
    result.index_revision = Some(crate::git::index::revision(&repository)?);
    if metadata(root)? != before {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "Git metadata changed during status inspection",
        ));
    }
    control.check()?;
    Ok(result)
}

fn parse(text: &str, truncated: bool, root: &Path, control: &Control) -> Result<GitStatus, Fault> {
    let mut result = GitStatus {
        kind: RepositoryKind::Unborn,
        branch: None,
        head: None,
        index_revision: None,
        entries: Vec::new(),
        truncated,
        omitted_paths: usize::from(truncated),
        repositories: Vec::new(),
    };
    let mut parents = EntryParents::new(path::root(root)?);
    let mut headers = 0;
    let complete = if truncated {
        text.rsplit_once('\0').map_or("", |(complete, _)| complete)
    } else if text.ends_with('\0') {
        text
    } else {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "incomplete Git status output",
        ));
    };
    for record in complete.split_terminator('\0') {
        control.check()?;
        if let Some(head) = record.strip_prefix("# branch.oid ") {
            headers += 1;
            if head != "(initial)" {
                result.head = Some(head.to_owned());
                result.kind = RepositoryKind::Ready;
            }
            continue;
        }
        if let Some(branch) = record.strip_prefix("# branch.head ") {
            headers += 1;
            result.branch = (branch != "(detached)").then(|| branch.to_owned());
            continue;
        }
        if record.starts_with("# ") {
            continue;
        }
        let mut entry = if let Some(path) = record.strip_prefix("? ") {
            classify(path.to_owned(), Status::WT_NEW).unwrap()
        } else {
            let fields = match record.as_bytes().first() {
                Some(b'1') => record.splitn(9, ' ').collect::<Vec<_>>(),
                Some(b'u') => record.splitn(11, ' ').collect::<Vec<_>>(),
                _ => {
                    return Err(Fault::new(
                        ErrorCode::Unavailable,
                        "invalid Git status output",
                    ));
                }
            };
            let count = if record.starts_with('u') { 11 } else { 9 };
            if fields.len() != count || fields[1].len() != 2 {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "incomplete Git status record",
                ));
            }
            let mut status = Status::empty();
            if record.starts_with('u') {
                status |= Status::CONFLICTED;
            } else {
                let code = fields[1].as_bytes();
                status |= change(code[0], true)? | change(code[1], false)?;
            }
            let Some(entry) = classify(fields[count - 1].to_owned(), status) else {
                continue;
            };
            entry
        };
        if valid_path(&entry.path).is_err() || parents.check(&entry.path, false).is_err() {
            result.omitted_paths += 1;
            continue;
        }
        if result.entries.len() == MAX_GIT_ENTRIES {
            result.truncated = true;
            result.omitted_paths += 1;
            continue;
        }
        entry.diff.truncated = true;
        entry.staged_diff.truncated = entry.staged.is_some() || entry.conflicted;
        entry.unstaged_diff.truncated =
            entry.unstaged.is_some() || entry.untracked || entry.conflicted;
        result.entries.push(entry);
    }
    if headers != 2 {
        return Err(Fault::new(
            ErrorCode::Unavailable,
            "incomplete Git status headers",
        ));
    }
    result
        .entries
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(result)
}

#[cfg(test)]
mod tests;

fn change(code: u8, staged: bool) -> Result<Status, Fault> {
    let status = match (code, staged) {
        (b'.', _) => Status::empty(),
        (b'M', true) => Status::INDEX_MODIFIED,
        (b'M', false) => Status::WT_MODIFIED,
        (b'T', true) => Status::INDEX_TYPECHANGE,
        (b'T', false) => Status::WT_TYPECHANGE,
        (b'D', true) => Status::INDEX_DELETED,
        (b'D', false) => Status::WT_DELETED,
        (b'A', true) => Status::INDEX_NEW,
        _ => {
            return Err(Fault::new(
                ErrorCode::Unavailable,
                "unsupported Git status classification",
            ));
        }
    };
    Ok(status)
}
