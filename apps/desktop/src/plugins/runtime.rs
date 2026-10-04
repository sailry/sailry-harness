use super::{cache::Cache, host::Host};
use gpui_kit::{App, AppContext as _, Entity, IntoElement, Styled, Window};
use gpui_shell::{ShellRoot, ShellRuntime, policy::Policy};
use std::{rc::Rc, sync::Arc};

pub(super) struct Mounted {
    #[cfg(test)]
    pub conversations: super::conversation::Conversations,
    pub terminals: super::terminals::Terminals,
    host: Arc<Host>,
    pub root: Entity<ShellRoot>,
    pub header: Entity<super::header::Header>,
    _runtime: Rc<ShellRuntime>,
    _cache: Cache,
    _activation: Entity<super::window::Activation>,
    _composer: Option<Entity<super::composer::State>>,
    _location: Entity<super::location::State>,
}

pub(super) struct Scopes {
    pub owner: gpui_kit::WeakEntity<super::Panel>,
    pub conversation: Option<super::conversation::Scope>,
    pub composer: super::composer::Scope,
    pub location: super::location::Scope,
    pub terminal: Option<super::terminals::Scope>,
    pub browser: super::browser::Scope,
    pub pane: super::panes::Scope,
    pub workspace: Option<Entity<super::workspace::State>>,
    pub resource: Option<Entity<super::resource::State>>,
    pub documents: super::documents::Scope,
    pub artifact: Option<super::previews::Resource>,
    pub usage_sources: super::usage::Sources,
    pub activity_sources: super::host::activity::Sources,
    pub file_transfers: super::file_transfers::Scope,
}

impl Mounted {
    pub(super) fn host(&self) -> Arc<Host> {
        self.host.clone()
    }

    pub(super) fn active(&self) -> bool {
        self.host.check().is_ok()
    }

    pub(super) fn observe(&self, cursor: u64, connected: bool) {
        self.host.observe(cursor, connected);
    }

    pub(super) fn close(&self) {
        self.host.close();
    }

    #[cfg(test)]
    pub(super) fn cache_root(&self) -> &std::path::Path {
        self._cache.root()
    }

    pub(super) fn new(
        cache: Cache,
        host: Arc<Host>,
        application: &str,
        scopes: Scopes,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui_shell::anyhow::Result<Self> {
        let Scopes {
            owner,
            conversation,
            composer,
            location,
            terminal,
            browser,
            pane,
            workspace,
            resource,
            documents,
            artifact,
            usage_sources,
            activity_sources,
            file_transfers,
        } = scopes;
        let runtime = gpui_component_shell::new_isolated_runtime()?;
        // Kit b79f4ce resolves script images through the application-wide asset
        // source. Mounted plugins need package-local paths instead. Keep Kit's
        // native image renderer and admit only declared SVG and validated PNG resources.
        let images = cache.images.clone();
        let tabs = super::controls::tabs::Controls::new(host.stop_token());
        let (header, with_header) = super::header::create(tabs.clone(), cx);
        let conversations = super::conversation::Conversations::default();
        let terminals = super::terminals::Terminals::default();
        let module = terminals
            .module(
                conversations.module(
                    with_header(host.module()),
                    conversation,
                    host.stop_token(),
                    cx,
                ),
                terminal,
                host.clone(),
            )
            .function("theme", |_| {
                gpui_shell::with_current_app(|cx| super::theme::snapshot(cx)).ok_or_else(|| {
                    gpui_shell::HostError::new("plugin theme requires an active view")
                })
            })
            .component("Image", move |args, _, _| {
                match args
                    .props()
                    .get("path")
                    .and_then(|path| path.as_str())
                    .and_then(|path| images.get(path))
                {
                    Some(path) => {
                        let mut image = gpui_kit::img(path.clone()).size_full();
                        // Div overflow clips rectangular bounds; the image renderer
                        // applies rounded corners to the actual texture.
                        if args
                            .props()
                            .get("circular")
                            .and_then(|value| value.as_bool())
                            == Some(true)
                        {
                            image = image.rounded_full();
                        }
                        #[cfg(test)]
                        {
                            use gpui_kit::InteractiveElement as _;
                            image = image.debug_selector(|| args.id().to_owned());
                        }
                        image.into_any_element()
                    }
                    None => gpui_kit::div().into_any_element(),
                }
            });
        let module = super::browser::surface(module, &browser, host.clone());
        #[cfg(test)]
        let anchors = gpui_shell::HostModule::new("sailry/test")
            .component("Anchor", |mut args, _, _| {
                use gpui_kit::{InteractiveElement as _, ParentElement as _};
                gpui_kit::div()
                    .debug_selector(|| args.id().to_owned())
                    .children(args.take_children())
                    .into_any_element()
            })
            .component("Bounds", |args, _, _| {
                use gpui_kit::{InteractiveElement as _, Styled as _};
                // A transparent absolute probe measures its relative parent
                // without replacing the parent's script styling or controls.
                gpui_kit::div()
                    .absolute()
                    .inset_0()
                    .debug_selector(|| args.id().to_owned())
                    .into_any_element()
            });
        let ui = super::notifications::module(
            super::navigation::ui(
                super::menu::module(super::overlay::module(cx), host.stop_token()),
                owner.clone(),
                host.clone(),
            ),
            owner.clone(),
            host.clone(),
        );
        let ui = super::browser::module(
            super::workspace::module(
                super::controls::module(ui, host.stop_token(), tabs, cx),
                workspace,
                host.clone(),
            ),
            browser,
            host.clone(),
            window,
            cx,
        );
        let ui = super::previews::module(ui, artifact, host.clone(), cx);
        let ui = super::native_context::module(ui, host.stop_token(), cx);
        let drops = super::tree::drops::Drops::new(host.stop_token());
        let ui = super::tree::module(ui, host.stop_token(), drops.clone(), cx);
        let ui = super::diff::module(ui, host.clone());
        let ui = super::resource::module(ui, resource, host.clone());
        let ui = super::picker::module(ui, host.stop_token(), cx);
        let ui = super::data_table::module(ui, host.stop_token(), cx);
        let ui = super::charts::module(ui);
        let ui = super::activity::module(ui, owner.clone(), host.clone());
        let sdk =
            host.sdk_with_documents(documents.controller.clone(), documents.write, usage_sources);
        let sdk = host.activity_module(sdk, activity_sources);
        let credentials = cx.new(|_| super::credentials::Store::default());
        let connections = host.connections_module(credentials.clone());
        let settings = host.settings_module(credentials.clone());
        let ssh_transfers = super::ssh_transfers::module(owner.clone(), host.clone(), drops, cx);
        let file_transfers = super::file_transfers::module(file_transfers, host.clone(), cx);
        let documents = super::documents::module(documents, host.clone(), cx);
        let (ui, composer) = super::composer::extend(ui, composer, host.clone(), window, cx);
        let (ui, location) =
            super::location::extend(ui, location, owner.clone(), host.clone(), window, cx);
        let (ui, activation) = super::window::extend(ui, host.stop_token(), window, cx);
        let policy = Policy::new()
            .with_application(application)
            .with_capabilities(
                gpui_shell::Capabilities::new().read_roots([cache.root().to_path_buf()]),
            )
            .with_host_module(module)?
            .with_host_module(sdk)?
            .with_host_module(connections)?
            .with_host_module(settings)?
            .with_host_module(ssh_transfers)?
            .with_host_module(super::credentials::module(credentials))?
            .with_host_module(file_transfers)?
            .with_host_module(documents)?
            .with_host_module(super::forms::module())?
            .with_host_module(super::panes::module(ui, owner, host.clone(), pane))?;
        #[cfg(test)]
        let policy = policy.with_host_module(anchors)?;
        // In b79f4ce, try_load captures the default policy synchronously and
        // carries it on every later call. No await or per-frame policy switch.
        // The public loader has no explicit-policy parameter at this revision.
        let mut previous = None;
        gpui_shell::policy::update_default(|current| {
            previous = Some(current);
            policy
        });
        let _reset = DefaultPolicy(previous);
        let root = runtime.try_load(cache.root(), window, cx)?;
        Ok(Self {
            #[cfg(test)]
            conversations,
            terminals,
            host,
            root,
            header,
            _runtime: runtime,
            _cache: cache,
            _activation: activation,
            _composer: composer,
            _location: location,
        })
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        self.host.close();
    }
}

struct DefaultPolicy(Option<Policy>);

impl Drop for DefaultPolicy {
    fn drop(&mut self) {
        gpui_shell::policy::set_default(self.0.take().expect("previous shell policy"));
    }
}
