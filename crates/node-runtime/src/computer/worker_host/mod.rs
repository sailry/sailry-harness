//! AppKit host for the pinned CUA private-worker transport.
//!
//! CUA's overlay owns NSApplication::run, so it cannot share GPUI's event loop.
//! Both application binaries reuse this entry point in a directly supervised
//! child; it opens no Node profile, listener, or second business runtime.
//! Protocol reference: trycua/cua 58aba84, cua-driver/src/private_worker.rs (MIT).
mod channel;

use cua_driver_sdk::{CuaDriver, DriverHostOptions, worker::WorkerInitialization};
use serde_json::json;
use std::io::{self, BufReader};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run_if_requested() -> bool {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(std::ffi::OsStr::new("__private-worker")) {
        return false;
    }
    let generation = match (args.next(), args.next(), args.next()) {
        (Some(flag), Some(value), None) if flag == "--generation" => value.into_string().ok(),
        _ => None,
    }
    .filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
    });
    let Some(generation) = generation else {
        eprintln!("computer worker requires a valid generation");
        std::process::exit(1);
    };
    let (ready, initialized) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(Box::<dyn std::error::Error>::from)
            .and_then(|runtime| runtime.block_on(serve(&generation, ready)));
        let code = match result {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("computer worker: {error}");
                1
            }
        };
        // EOF/shutdown owns this child's lifetime, including AppKit's run loop.
        std::process::exit(code);
    });
    if initialized.recv().is_ok() {
        cua_platform_macos::cursor::overlay::run_on_main_thread();
    }
    true
}

async fn serve(generation: &str, ready: std::sync::mpsc::SyncSender<()>) -> Result<()> {
    let mut reader = BufReader::new(io::stdin().lock());
    let mut writer = io::stdout().lock();
    let Some(request) = channel::read(&mut reader, generation)? else {
        return Ok(());
    };
    if request.operation != "initialize"
        || request.request_id != 0
        || request.session_handle.is_some()
    {
        return Err("computer worker requires initialization first".into());
    }
    let initialization: WorkerInitialization = serde_json::from_value(
        request
            .arguments
            .ok_or("computer worker initialization is missing")?,
    )?;
    let driver = CuaDriver::try_create_configured_for_host(
        initialization.configured_driver,
        DriverHostOptions {
            cursor: Default::default(),
            host_owns_permission_ux: true,
            host_bundle_id: Some(initialization.host_bundle_id.clone()),
            claude_code_compatibility: false,
            prepare_desktop_environment: true,
            register_host_tools: None,
            authorization_host: None,
            activity_observer: None,
        },
    )?;
    ready.send(())?;
    channel::write(
        &mut writer,
        channel::Response::ok(
            0,
            generation,
            json!({
                "ready":true,
                "pid":std::process::id(),
                "host_bundle_id":initialization.host_bundle_id,
                "metadata":driver.metadata().await?,
            }),
        ),
    )?;
    let mut sessions = channel::Sessions::new();
    while let Some(request) = channel::read(&mut reader, generation)? {
        let shutdown = request.operation == "shutdown";
        let response = channel::execute(&driver, &mut sessions, generation, request).await;
        channel::write(&mut writer, response)?;
        if shutdown {
            return Ok(());
        }
    }
    sessions.clear();
    driver.shutdown().await?;
    Ok(())
}
