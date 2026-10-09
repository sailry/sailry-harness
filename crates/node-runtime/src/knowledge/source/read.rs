use super::*;
use std::{io::Read, time::SystemTime};

pub(super) fn document(
    target: Target,
    path: String,
    cancel: &CancellationToken,
) -> Result<SourceDoc, Fault> {
    cancelled(cancel)?;
    let mut file = files::open_regular(&target.directory, &target.name)?;
    let before = file.metadata().map_err(files::io_error)?;
    if before.len() > crate::office::LIMIT as u64 {
        return Err(Fault::invalid("knowledge source exceeds 32 MiB"));
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = file.read(&mut buffer).map_err(files::io_error)?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > crate::office::LIMIT {
            return Err(Fault::invalid("knowledge source exceeds 32 MiB"));
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let after = file.metadata().map_err(files::io_error)?;
    if bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.modified().map_err(files::io_error)?
            != after.modified().map_err(files::io_error)?
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "knowledge source changed during extraction",
        ));
    }
    let modified_ms = u64::try_from(
        before
            .modified()
            .map_err(files::io_error)?
            .into_std()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|_| Fault::invalid("knowledge source modification time predates the epoch"))?
            .as_millis(),
    )
    .map_err(|_| Fault::invalid("knowledge source modification time is out of range"))?;
    let extension = target
        .name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let parts = if matches!(extension.as_str(), "pdf" | "docx" | "xlsx" | "pptx") {
        crate::office::parts(&target.name, &bytes)?
            .into_iter()
            .map(|part| Part {
                title: part.name,
                text: part.text,
                position: part.position,
            })
            .collect()
    } else {
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Fault::invalid("knowledge source is binary or unsupported text"))?;
        if text.chars().any(|character| {
            character.is_control() && !matches!(character, '\n' | '\r' | '\t' | '\u{c}')
        }) {
            return Err(Fault::invalid(
                "knowledge source is binary or unsupported text",
            ));
        }
        let lines = u32::try_from(text.lines().count().max(1))
            .map_err(|_| Fault::invalid("knowledge source has too many lines"))?;
        vec![Part {
            title: target.name,
            text: text.to_owned(),
            position: Position::Lines {
                start: 1,
                end: lines,
            },
        }]
    };
    cancelled(cancel)?;
    Ok(SourceDoc {
        path,
        revision: blake3::hash(&bytes).to_hex().to_string(),
        modified_ms,
        parts,
    })
}
