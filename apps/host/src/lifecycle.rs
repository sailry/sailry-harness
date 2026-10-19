//! Every post-start exit awaits the existing Node shutdown owner.
use super::{bootstrap, options::Options, pairing};
use sailry_node_runtime::Node;
use std::{error::Error, future::Future};

type Failure = Box<dyn Error>;

pub(super) async fn serve(
    node: Node,
    options: Options,
    stopped: impl Future<Output = Result<(), Failure>>,
) -> Result<(), Failure> {
    let result = active(&node, options, stopped).await;
    let shutdown = node.shutdown().await;
    if let Err(error) = result {
        if let Err(error) = shutdown {
            eprintln!("Host shutdown failed: {error}");
        }
        return Err(error);
    }
    shutdown?;
    println!("Node stopped");
    Ok(())
}

async fn active(
    node: &Node,
    options: Options,
    stopped: impl Future<Output = Result<(), Failure>>,
) -> Result<(), Failure> {
    if options.internet {
        node.link().online().await?;
    }
    node.observe().probe().await?;
    let stop = sailry_link::CancellationToken::new();
    #[cfg(unix)]
    let mut control = None;
    let mut bootstrap = None;
    let mut sharing = None;
    let result = async {
        #[cfg(unix)]
        {
            let listener = super::control::listen(&options.data_dir)?;
            control = Some(tokio::spawn(super::control::serve(
                listener,
                node.link(),
                node.local(),
                stop.clone(),
            )));
        }
        println!(
            "Node ready: {} (only paired peers are authorized)",
            node.profile().display()
        );
        println!(
            "Link address: {}",
            serde_json::to_string(&node.link().address())?
        );
        if options.bootstrap {
            let path = options.data_dir.join("bootstrap.ticket");
            let link = node.link();
            let stop = stop.clone();
            bootstrap = Some(tokio::spawn(async move {
                if let Err(error) = bootstrap::serve(link, path, stop).await {
                    eprintln!("Host bootstrap failed: {error}");
                }
            }));
        }
        if let Some(relay) = options.pairing {
            sharing = Some(tokio::spawn(pairing::share(
                relay,
                node.link(),
                stop.clone(),
            )));
        }
        stopped.await
    }
    .await;
    stop.cancel();
    #[cfg(unix)]
    let socket = if let Some(control) = control {
        let _ = control.await;
        std::fs::remove_file(options.data_dir.join("control.sock"))
    } else {
        Ok(())
    };
    if let Some(sharing) = sharing {
        let _ = sharing.await;
    }
    if let Some(bootstrap) = bootstrap {
        let _ = bootstrap.await;
    }
    result?;
    #[cfg(unix)]
    socket?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(path: &std::path::Path) -> Options {
        Options {
            data_dir: path.into(),
            network: Default::default(),
            internet: false,
            bootstrap: false,
            pairing: None,
        }
    }
    #[tokio::test]
    async fn signal_failure_closes_owned_node() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        let error = serve(node, options(&profile), async {
            Err("signal observer failed".into())
        })
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "signal observer failed");
        #[cfg(unix)]
        assert!(!profile.join("control.sock").exists());
        Node::start_empty(&profile)
            .await
            .unwrap()
            .shutdown()
            .await
            .unwrap();
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn preserves_control_startup_failure() {
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("node");
        let node = Node::start_empty(&profile).await.unwrap();
        std::fs::write(profile.join("control.sock"), b"preserve").unwrap();
        let error = serve(node, options(&profile), std::future::pending())
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "Host control path is not a socket");
        assert_eq!(
            std::fs::read(profile.join("control.sock")).unwrap(),
            b"preserve"
        );
        Node::start_empty(&profile)
            .await
            .unwrap()
            .shutdown()
            .await
            .unwrap();
    }
}
