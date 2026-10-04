use super::*;
use std::process::Stdio;
use tokio::io::AsyncReadExt;

pub(super) async fn run(
    root: PathBuf,
    command: String,
    mut channel: Channel<server::Msg>,
    handle: server::Handle,
) {
    let id = channel.id();
    let mut child = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let input = async {
        let _ = tokio::io::copy(&mut channel.make_reader(), &mut stdin).await;
        drop(stdin);
    };
    let output = async {
        let mut buffer = [0; 32768];
        while let Ok(count) = stdout.read(&mut buffer).await {
            if count == 0 || handle.data(id, buffer[..count].to_vec()).await.is_err() {
                break;
            }
        }
    };
    let errors = async {
        let mut buffer = [0; 4096];
        while let Ok(count) = stderr.read(&mut buffer).await {
            if count == 0
                || handle
                    .extended_data(id, 1, buffer[..count].to_vec())
                    .await
                    .is_err()
            {
                break;
            }
        }
    };
    let work = async {
        let finish = async {
            let (status, (), ()) = tokio::join!(child.wait(), output, errors);
            let _ = handle
                .exit_status_request(id, status.unwrap().code().unwrap_or(255) as u32)
                .await;
            let _ = handle.eof(id).await;
            let _ = handle.close(id).await;
        };
        tokio::join!(input, finish);
    };
    let _ = tokio::time::timeout(Duration::from_secs(120), work).await;
}
