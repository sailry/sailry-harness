//! Private service control reuses the running Node; it never opens another profile owner.
use sailry_link::{
    CancellationToken, LinkHandle,
    rendezvous::{DEFAULT_SERVICE, Relay, ShareState},
};
use std::{ffi::OsString, path::Path};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::watch,
    task::JoinSet,
};

type Error = Box<dyn std::error::Error + Send + Sync>;

#[cfg(test)]
mod tests;

pub(crate) fn listen(profile: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    let path = profile.join("control.sock");
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(&path)?,
        Ok(_) => return Err(std::io::Error::other("Host control path is not a socket")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

pub(crate) async fn serve(listener: UnixListener, link: LinkHandle, stop: CancellationToken) {
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let link = link.clone();
                    let stop = stop.clone();
                    clients.spawn(async move {
                        if let Err(error) = handle(stream, link, stop).await {
                            eprintln!("Host control failed: {error}");
                        }
                    });
                },
                Err(error) => { eprintln!("Host control listener failed: {error}"); break; },
            },
            _ = clients.join_next(), if !clients.is_empty() => {},
        }
    }
    clients.abort_all();
    while clients.join_next().await.is_some() {}
}

async fn handle(
    stream: UnixStream,
    link: LinkHandle,
    stop: CancellationToken,
) -> Result<(), Error> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader.take(8193));
    let mut request = String::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.read_line(&mut request),
    )
    .await??;
    let relay = request_relay(&request)?;
    let (sender, mut receiver) = watch::channel(ShareState::Preparing);
    let sharing = relay.share(&link, sender, stop);
    tokio::pin!(sharing);
    let mut disconnect = [0];
    loop {
        let value = response(&receiver.borrow_and_update());
        writer
            .write_all(serde_json::to_string(&value)?.as_bytes())
            .await?;
        writer.write_all(b"\n").await?;
        if matches!(value["state"].as_str(), Some("paired" | "closed")) {
            return Ok(());
        }
        tokio::select! {
            _ = reader.read(&mut disconnect) => return Ok(()),
            changed = receiver.changed() => if changed.is_err() { return Ok(()); },
            result = &mut sharing => {
                result?;
                let value = response(&receiver.borrow_and_update());
                writer.write_all(serde_json::to_string(&value)?.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                return Ok(());
            },
        }
    }
}

fn request_relay(request: &str) -> Result<Relay, Error> {
    if request.len() > 8192 || !request.ends_with('\n') {
        return Err("Invalid Host control request".into());
    }
    let value: serde_json::Value = serde_json::from_str(request)?;
    let fields = value.as_object().ok_or("Invalid Host control request")?;
    if value["version"] != 1
        || value["command"] != "share"
        || fields
            .keys()
            .any(|key| !matches!(key.as_str(), "version" | "command" | "origin"))
    {
        return Err("Unsupported Host control request".into());
    }
    Ok(Relay::new(
        value["origin"]
            .as_str()
            .ok_or("Missing pairing service origin")?,
    )?)
}

fn response(state: &ShareState) -> serde_json::Value {
    use serde_json::json;
    match state {
        ShareState::Preparing => json!({"version":1,"state":"preparing"}),
        ShareState::Ready {
            code,
            expires_at_ms,
        } => json!({"version":1,"state":"ready","code":code,"expires_at_ms":expires_at_ms}),
        ShareState::Retrying(_) => json!({"version":1,"state":"retrying"}),
        ShareState::Paired => json!({"version":1,"state":"paired"}),
        ShareState::Closed => json!({"version":1,"state":"closed"}),
    }
}

pub(crate) async fn share(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = arguments;
    let mut profile = None;
    let mut origin = None;
    while let Some(argument) = arguments.next() {
        if argument == "--data-dir" && profile.is_none() {
            let path =
                std::path::PathBuf::from(arguments.next().ok_or("--data-dir requires a path")?);
            if !path.is_absolute() {
                return Err("--data-dir must be absolute".into());
            }
            profile = Some(path);
        } else if argument == "--pairing-service" && origin.is_none() {
            origin = Some(
                arguments
                    .next()
                    .ok_or("--pairing-service requires an origin")?
                    .into_string()
                    .map_err(|_| "Pairing origin must be UTF-8")?,
            );
        } else {
            return Err("Usage: sailry share [--pairing-service <HTTPS origin>]".into());
        }
    }
    let profile = profile.unwrap_or(sailry_node_runtime::default_data_dir()?);
    let origin = origin.as_deref().unwrap_or(DEFAULT_SERVICE);
    Relay::new(origin)?;
    let stream = UnixStream::connect(profile.join("control.sock"))
        .await
        .map_err(|_| "Host is not running; use sailry start")?;
    let (reader, mut writer) = stream.into_split();
    let request = serde_json::json!({"version":1,"command":"share","origin":origin});
    writer
        .write_all(serde_json::to_string(&request)?.as_bytes())
        .await?;
    writer.write_all(b"\n").await?;
    let mut lines = BufReader::new(reader).lines();
    loop {
        let line = tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            line = lines.next_line() => line?.ok_or("Host sharing stopped")?,
        };
        let value: serde_json::Value = serde_json::from_str(&line)?;
        if value["version"] != 1 {
            return Err("Unsupported Host control response".into());
        }
        match value["state"].as_str() {
            Some("ready") => println!(
                "Pairing PIN: {} (expires at Unix ms {}); keep private",
                value["code"].as_str().ok_or("Invalid pairing code")?,
                value["expires_at_ms"]
                    .as_u64()
                    .ok_or("Invalid pairing expiry")?
            ),
            Some("retrying") => eprintln!("Pairing service unavailable; retrying"),
            Some("paired") => {
                println!("Pairing complete");
                return Ok(());
            }
            Some("closed") => return Ok(()),
            Some("preparing") => {}
            _ => return Err("Invalid Host control response".into()),
        }
    }
}
