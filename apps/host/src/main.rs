#[cfg(unix)]
mod acp;
mod bootstrap;
#[cfg(unix)]
mod control;
#[cfg(all(test, unix))]
mod history_fixture;
mod lifecycle;
#[cfg(all(test, unix))]
#[path = "../../../crates/node-runtime/tests/agent_support/mod.rs"]
#[allow(dead_code)]
mod models;
mod options;
mod pairing;

use sailry_node_runtime::Node;

fn main() -> std::process::ExitCode {
    let result = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(Box::<dyn std::error::Error>::from)
        .and_then(|runtime| runtime.block_on(run()));
    match result {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("sailry-host: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<u8, Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.as_slice() == ["--version"] || arguments.as_slice() == ["version"] {
        println!("Sailry Host {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    #[cfg(unix)]
    if arguments
        .first()
        .is_some_and(|argument| argument == "share")
    {
        return control::share(arguments.into_iter().skip(1))
            .await
            .map(|()| 0);
    }
    if arguments
        .first()
        .is_some_and(|argument| argument == "submit" || argument == "wait")
    {
        #[cfg(unix)]
        return control::task::run(arguments.into_iter()).await;
        #[cfg(not(unix))]
        return Err("Task management requires a Unix Host control socket".into());
    }
    if arguments.first().is_some_and(|argument| argument == "acp") {
        #[cfg(unix)]
        return acp::run(arguments.into_iter().skip(1)).await.map(|()| 0);
        #[cfg(not(unix))]
        return Err("ACP management requires a Unix Host control socket".into());
    }
    let Some(options) = options::parse(arguments.into_iter())? else {
        return Ok(0);
    };
    let _logging = match sailry_node_runtime::logging::install(
        &options.data_dir,
        sailry_node_runtime::logging::Application::Host,
    ) {
        Ok(guard) => Some(guard),
        Err(_) => {
            eprintln!("sailry-host: logging is unavailable");
            None
        }
    };
    // Install handlers before opening a profile or announcing readiness.
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let node = Node::start_with_network(&options.data_dir, options.network.clone()).await?;
    let stopped = async move {
        #[cfg(unix)]
        tokio::select! {
            _ = terminate.recv() => {},
            _ = interrupt.recv() => {},
        }
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await?;
        Ok(())
    };
    lifecycle::serve(node, options, stopped).await.map(|()| 0)
}
