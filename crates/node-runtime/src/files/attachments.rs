//! Immutable Node-owned files; display names never become storage paths.
mod inputs;
use super::{io_error, path, save};
use cap_fs_ext::DirExt;
use cap_std::fs::Dir;
pub(crate) use inputs::Inputs;
use sailry_protocol::{AttachmentId, ErrorCode, Fault, attachment};
use std::{collections::BTreeSet, path::Path};

pub(super) fn validate(spec: &attachment::Spec) -> Result<(), Fault> {
    let media = spec.media_type.split_once('/');
    let token = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-".contains(&byte))
    };
    if spec.name.is_empty()
        || spec.name.len() > 1024
        || spec.name.contains(['/', '\\'])
        || spec.name.chars().any(char::is_control)
        || spec.media_type.len() > 127
        || !media.is_some_and(|(kind, subtype)| token(kind) && token(subtype))
        || spec.size > attachment::MAX_BYTES
    {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "invalid attachment metadata",
        ));
    }
    Ok(())
}

pub(super) fn prepare(
    profile: Option<&Path>,
    spec: attachment::Spec,
) -> Result<(attachment::Attachment, save::Target), Fault> {
    let profile = profile.ok_or_else(unavailable)?;
    let dir = path::root(profile)?;
    match dir.create_dir("attachments") {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    dir.open_dir_nofollow("attachments").map_err(io_error)?;
    let attachment = attachment::Attachment {
        id: AttachmentId::new(),
        spec,
    };
    let target = save::Target::prepare(
        &profile.join("attachments"),
        &attachment.id.to_string(),
        None,
        attachment::MAX_BYTES,
        None,
    )?;
    Ok((attachment, target))
}

pub(super) fn open(
    profile: Option<&Path>,
    attachment: &attachment::Attachment,
) -> Result<std::fs::File, Fault> {
    let dir = directory(profile.ok_or_else(unavailable)?)?;
    let file = super::open_regular(&dir, &attachment.id.to_string())?;
    if file.metadata().map_err(io_error)?.len() != attachment.spec.size {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "attachment content changed",
        ));
    }
    Ok(file.into_std())
}

/// Runs only before admission or after the resource worker drains.
pub(crate) fn collect(profile: &Path, retained: &BTreeSet<AttachmentId>) -> Result<(), Fault> {
    let dir = match directory(profile) {
        Ok(dir) => dir,
        Err(error) if error.code == ErrorCode::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in dir.entries().map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Ok(id) = name.parse::<AttachmentId>() else {
            continue;
        };
        if id.to_string() == name
            && !retained.contains(&id)
            && entry.file_type().map_err(io_error)?.is_file()
        {
            dir.remove_file(name).map_err(io_error)?;
        }
    }
    Ok(())
}

fn directory(profile: &Path) -> Result<Dir, Fault> {
    path::root(profile)?
        .open_dir_nofollow("attachments")
        .map_err(io_error)
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "attachment storage is unavailable")
}

impl super::Files {
    pub(crate) async fn attachment(
        &self,
        profile: Option<std::path::PathBuf>,
        attachment: attachment::Attachment,
        stop: sailry_link::CancellationToken,
    ) -> Result<Vec<u8>, Fault> {
        let permit = tokio::select! {
            _ = stop.cancelled() => return Err(Fault::new(ErrorCode::Cancelled, "attachment read cancelled")),
            permit = tokio::time::timeout(std::time::Duration::from_secs(5), self.capacity.clone().acquire_owned()) => permit
                .map_err(|_| Fault::new(ErrorCode::Busy, "attachment read capacity deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Unavailable, "attachment reader is unavailable"))?,
        };
        let cancelled = stop.child_token();
        let _guard = cancelled.clone().drop_guard();
        let worker = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let control = super::Control {
                cancelled,
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
            };
            control.check()?;
            let mut file = open(profile.as_deref(), &attachment)?;
            let bytes = super::read_bytes(&mut file, attachment.spec.size as usize + 1, &control)?;
            if bytes.len() as u64 != attachment.spec.size
                || blake3::hash(&bytes).to_hex().as_str() != attachment.spec.revision
            {
                return Err(Fault::new(
                    ErrorCode::RevisionConflict,
                    "attachment content changed",
                ));
            }
            Ok(bytes)
        });
        tokio::select! {
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "attachment read cancelled")),
            result = tokio::time::timeout(std::time::Duration::from_secs(5), worker) => result
                .map_err(|_| Fault::new(ErrorCode::Busy, "attachment read deadline exceeded"))?
                .map_err(|_| Fault::new(ErrorCode::Internal, "attachment worker failed"))?,
        }
    }
}

#[cfg(test)]
mod tests;
