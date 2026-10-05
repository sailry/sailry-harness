//! Desktop owns presentation and the process executor; Node owns service startup.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::App;
use gpui_kit::Global;
use sailry_link::LinkHandle;
use sailry_node_runtime::{NetworkScope, Node};

#[derive(Clone)]
pub struct Services {
    pub runtime: Arc<tokio::runtime::Runtime>,
    pub link: LinkHandle,
    pub local: Arc<dyn sailry_link::Transport>,
    pub relay_enabled: bool,
}
impl Global for Services {}

/// Both normal quit and an updater handoff drain the same local Node owner.
#[derive(Clone)]
pub(crate) struct Lifecycle {
    owner: Arc<tokio::sync::Mutex<Option<Node>>>,
    profile: PathBuf,
}
impl Global for Lifecycle {}

impl Lifecycle {
    fn new(node: Node) -> Self {
        Self {
            profile: node.profile().to_path_buf(),
            owner: Arc::new(tokio::sync::Mutex::new(Some(node))),
        }
    }

    pub(crate) fn profile(&self) -> &Path {
        &self.profile
    }

    pub(crate) async fn stop(&self) -> Result<(), String> {
        let mut owner = self.owner.lock().await;
        if let Some(node) = owner.take() {
            node.shutdown().await.map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

pub struct Owner {
    services: Services,
    node: Node,
    #[cfg(unix)]
    signals: (tokio::signal::unix::Signal, tokio::signal::unix::Signal),
}

impl Owner {
    pub fn start(options: crate::startup::Options) -> Result<Self, Box<dyn std::error::Error>> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
        );
        let relay_enabled = true;
        #[cfg(unix)]
        let signals = {
            use tokio::signal::unix::{SignalKind, signal};
            let _entered = runtime.enter();
            (
                signal(SignalKind::terminate())?,
                signal(SignalKind::interrupt())?,
            )
        };
        let preferences = crate::preferences::Preferences::open(
            options.data_dir.join("desktop/preferences.json"),
        );
        let relays = if options.relays.is_empty() {
            preferences.data.iroh_relays.clone().unwrap_or_default()
        } else {
            options.relays
        };
        let scope = if relays.is_empty() {
            NetworkScope::Internet
        } else {
            NetworkScope::CustomRelays(relays)
        };
        let computer = if cfg!(target_os = "macos") {
            Some(sailry_node_runtime::ComputerWorker {
                executable: std::env::current_exe()?,
                bundle_id: "ai.sailry.desktop".into(),
            })
        } else {
            None
        };
        let node =
            runtime.block_on(Node::start_with_computer(options.data_dir, scope, computer))?;
        Ok(Self {
            services: Services {
                runtime,
                link: node.link(),
                local: node.local(),
                relay_enabled,
            },
            node,
            #[cfg(unix)]
            signals,
        })
    }

    pub fn install(self, cx: &mut App) {
        cx.set_global(crate::preferences::Preferences::open(
            self.node.profile().join("desktop/preferences.json"),
        ));
        let lifecycle = Lifecycle::new(self.node);
        cx.set_global(lifecycle.clone());
        let runtime = self.services.runtime.clone();
        #[cfg(unix)]
        {
            let (mut terminate, mut interrupt) = self.signals;
            let signal = runtime.spawn(async move {
                tokio::select! {
                    _ = terminate.recv() => {},
                    _ = interrupt.recv() => {},
                }
            });
            cx.spawn(async move |cx| {
                if signal.await.is_ok() {
                    cx.update(|cx| cx.quit());
                }
            })
            .detach();
        }
        cx.set_global(self.services);
        cx.on_app_quit(move |_| {
            // GPUI 0.3.4 limits returned quit futures to 200ms. At final process
            // exit, wait on the separate Tokio runtime for Node's bounded drain
            // before returning; ordinary window work never blocks this way.
            match runtime.block_on(lifecycle.stop()) {
                Ok(()) => println!("Desktop Node stopped"),
                Err(error) => eprintln!("desktop Node shutdown failed: {error}"),
            }
            std::future::ready(())
        })
        .detach();
        println!("Desktop Node ready");
    }
}

#[cfg(test)]
mod tests;
