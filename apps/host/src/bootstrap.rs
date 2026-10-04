//! One-time SSH handoff. Tickets stay in the private profile, never in service logs.
use sailry_link::{CancellationToken, LinkHandle};
use std::{path::PathBuf, time::Duration};

pub(crate) async fn serve(
    link: LinkHandle,
    path: PathBuf,
    stop: CancellationToken,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !link.peers().await?.is_empty() {
        return Ok(());
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    let result = async {
        loop {
            let mut invitation = link.invite()?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let temporary =
                path.with_extension(format!("{}.tmp", sailry_protocol::RequestId::new()));
            let mut file = options.open(&temporary)?;
            use std::io::Write;
            file.write_all(invitation.ticket().as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(temporary, &path)?;
            let paired = tokio::select! {
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep_until(deadline) => break,
                result = invitation.paired() => result.is_ok(),
            };
            std::fs::remove_file(&path)?;
            if paired {
                break;
            }
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    // Only our fixed, private handoff file is removed. The profile is preserved.
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    result
}
