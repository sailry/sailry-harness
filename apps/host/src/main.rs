mod bootstrap;
#[cfg(target_os = "macos")]
#[path = "../../../crates/node-runtime/src/computer/worker_host/mod.rs"]
mod computer_worker;
#[cfg(unix)]
mod control;
mod options;
mod pairing;

use sailry_node_runtime::Node;

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "macos")]
    if computer_worker::run_if_requested() {
        return std::process::ExitCode::SUCCESS;
    }
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(|runtime| runtime.block_on(run()));
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sailry-host: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.as_slice() == ["--version"] || arguments.as_slice() == ["version"] {
        println!("Sailry Host {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(unix)]
    if arguments
        .first()
        .is_some_and(|argument| argument == "share")
    {
        return control::share(arguments.into_iter().skip(1)).await;
    }
    let Some(options) = options::parse(arguments.into_iter())? else {
        return Ok(());
    };
    // Install handlers before opening a profile or announcing readiness.
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let computer = if cfg!(target_os = "macos") {
        Some(sailry_node_runtime::ComputerWorker {
            executable: std::env::current_exe()?,
            bundle_id: "ai.sailry.host".into(),
        })
    } else {
        None
    };
    let node = Node::start_with_computer(&options.data_dir, options.network, computer).await?;
    if options.internet
        && let Err(error) = node.link().online().await
    {
        node.shutdown().await?;
        return Err(error.into());
    }
    node.observe().probe().await?;
    let stop = sailry_link::CancellationToken::new();
    #[cfg(unix)]
    let control = match control::listen(&options.data_dir) {
        Ok(listener) => listener,
        Err(error) => {
            node.shutdown().await?;
            return Err(error.into());
        }
    };
    #[cfg(unix)]
    let control = tokio::spawn(control::serve(control, node.link(), stop.clone()));
    println!(
        "Node ready: {} (only paired peers are authorized)",
        node.profile().display()
    );
    println!(
        "Link address: {}",
        serde_json::to_string(&node.link().address())?
    );
    let bootstrap = options.bootstrap.then(|| {
        let path = options.data_dir.join("bootstrap.ticket");
        let link = node.link();
        let stop = stop.clone();
        tokio::spawn(async move {
            if let Err(error) = bootstrap::serve(link, path, stop).await {
                eprintln!("Host bootstrap failed: {error}");
            }
        })
    });
    let sharing = options
        .pairing
        .map(|relay| tokio::spawn(pairing::share(relay, node.link(), stop.clone())));
    #[cfg(unix)]
    tokio::select! {
        _ = terminate.recv() => {},
        _ = interrupt.recv() => {},
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    stop.cancel();
    #[cfg(unix)]
    {
        let _ = control.await;
        std::fs::remove_file(options.data_dir.join("control.sock"))?;
    }
    if let Some(sharing) = sharing {
        let _ = sharing.await;
    }
    if let Some(bootstrap) = bootstrap {
        let _ = bootstrap.await;
    }
    node.shutdown().await?;
    println!("Node stopped");
    Ok(())
}
