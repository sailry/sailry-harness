use super::*;
use sailry_protocol::ssh::FileAction;

async fn path(sftp: &SftpSession, value: &str) -> Result<String, Fault> {
    validate_path(value)?;
    let value = value.trim_end_matches('/');
    let (parent, name) = value.rsplit_once('/').unwrap_or((".", value));
    if name.is_empty() || name == "." || name == ".." {
        return Err(Fault::new(
            ErrorCode::InvalidRequest,
            "Select a file or directory",
        ));
    }
    let parent = sftp
        .canonicalize(if parent.is_empty() { "/" } else { parent })
        .await
        .map_err(error)?;
    Ok(join(&parent, name))
}
fn join(parent: &str, name: &str) -> String {
    format!("{}/{name}", parent.trim_end_matches('/'))
}
async fn absent(sftp: &SftpSession, path: &str) -> Result<(), Fault> {
    match sftp.symlink_metadata(path).await {
        Ok(_) => Err(Fault::new(
            ErrorCode::Conflict,
            "Remote destination already exists",
        )),
        Err(fault) => {
            let fault = error(fault);
            if fault.code == ErrorCode::NotFound {
                Ok(())
            } else {
                Err(fault)
            }
        }
    }
}
fn name(value: &str) -> bool {
    !value.is_empty() && !matches!(value, "." | "..") && !value.contains(['/', '\0'])
}

pub(super) async fn execute(
    sftp: &SftpSession,
    source: &str,
    action: FileAction,
) -> Result<(), Fault> {
    let source = path(sftp, source).await?;
    let copying = matches!(action, FileAction::Copy { .. });
    match action {
        FileAction::Create { directory } => {
            absent(sftp, &source).await?;
            if directory {
                sftp.create_dir(source).await.map_err(error)
            } else {
                sftp.open_with_flags(
                    source,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                )
                .await
                .map_err(error)?
                .shutdown()
                .await
                .map_err(io_error)
            }
        }
        FileAction::Move { destination } | FileAction::Copy { destination } => {
            let destination = path(sftp, &destination).await?;
            if source == destination || destination.starts_with(&(source.clone() + "/")) {
                return Err(Fault::new(
                    ErrorCode::InvalidRequest,
                    "Cannot place an entry inside itself",
                ));
            }
            absent(sftp, &destination).await?;
            if copying {
                copy(sftp, source, destination).await
            } else {
                sftp.rename(source, destination).await.map_err(error)
            }
        }
        FileAction::Remove => {
            let mut pending = vec![(source, false)];
            while let Some((path, visited)) = pending.pop() {
                if visited {
                    sftp.remove_dir(path).await.map_err(error)?;
                    continue;
                }
                let metadata = sftp.symlink_metadata(&path).await.map_err(error)?;
                if metadata.is_dir() {
                    pending.push((path.clone(), true));
                    for entry in sftp.read_dir(&path).await.map_err(error)? {
                        let entry = entry.file_name();
                        if name(&entry) {
                            pending.push((join(&path, &entry), false));
                        }
                    }
                } else {
                    sftp.remove_file(path).await.map_err(error)?;
                }
            }
            Ok(())
        }
    }
}
async fn copy(sftp: &SftpSession, source: String, destination: String) -> Result<(), Fault> {
    let mut pending = vec![(source, destination)];
    while let Some((source, destination)) = pending.pop() {
        let metadata = sftp.symlink_metadata(&source).await.map_err(error)?;
        if metadata.is_dir() {
            sftp.create_dir(&destination).await.map_err(error)?;
            for entry in sftp.read_dir(&source).await.map_err(error)? {
                let entry = entry.file_name();
                if name(&entry) {
                    pending.push((join(&source, &entry), join(&destination, &entry)));
                }
            }
        } else if metadata.is_symlink() {
            let target = sftp.read_link(&source).await.map_err(error)?;
            sftp.symlink(target, destination).await.map_err(error)?;
        } else if metadata.file_type().is_file() {
            let mut input = sftp.open(&source).await.map_err(error)?;
            let mut output = sftp
                .open_with_flags(
                    destination,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                )
                .await
                .map_err(error)?;
            tokio::io::copy(&mut input, &mut output)
                .await
                .map_err(|_| unknown("Remote copy interrupted; inspect the destination"))?;
            output.shutdown().await.map_err(io_error)?;
        } else {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "Unsupported remote file type",
            ));
        }
    }
    Ok(())
}
