use super::*;
use gpui_shell::{ShellRoot, ShellRuntime, policy::Policy};
use std::rc::Rc;

pub(super) struct Mounted {
    pub root: Entity<ShellRoot>,
    pub host: Arc<host::Host>,
    _runtime: Rc<ShellRuntime>,
    _cache: Arc<super::super::cache::Cache>,
}

impl Mounted {
    pub(super) fn new(
        cache: Arc<super::super::cache::Cache>,
        host: Arc<host::Host>,
        owner: WeakEntity<Card>,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui_shell::anyhow::Result<Self> {
        let runtime = gpui_component_shell::new_isolated_runtime()?;
        let policy = Policy::new()
            .with_application(&format!(
                "card-{}-{}",
                host.summary.receipt.call, host.summary.receipt.block
            ))
            .with_capabilities(
                gpui_shell::Capabilities::new().read_roots([cache.root().to_owned()]),
            )
            .with_host_module(host.module(owner))?;
        let mut previous = None;
        gpui_shell::policy::update_default(|current| {
            previous = Some(current);
            policy
        });
        let _reset = Reset(previous);
        let root = runtime.try_load(cache.root(), window, cx)?;
        Ok(Self {
            root,
            host,
            _runtime: runtime,
            _cache: cache,
        })
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.host.close();
    }
}

struct Reset(Option<Policy>);
impl Drop for Reset {
    fn drop(&mut self) {
        gpui_shell::policy::set_default(self.0.take().expect("previous card policy"));
    }
}
