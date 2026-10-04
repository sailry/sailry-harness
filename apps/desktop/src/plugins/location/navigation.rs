//! Captured UI navigation reuses the core document and project entry points.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectCreation {
    #[serde(default)]
    clone: bool,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    path: Option<String>,
}

pub(super) fn module(
    module: HostModule,
    state: Entity<State>,
    owner: WeakEntity<Panel>,
    host: Arc<Host>,
) -> HostModule {
    let declarations = format!(
        "{}\nexport function openFile(path:string,line?:number):Promise<boolean>;\nexport function openProjectCreation(options:{{clone?:boolean}}):Promise<boolean>;\nexport function openResource(kind:'git',target?:{{path?:string|null}}):Promise<boolean>;",
        module.declared().unwrap_or_default()
    );
    let file_owner = owner.clone();
    let file_state = state.clone();
    let file_host = host.clone();
    let resource_owner = owner.clone();
    let resource_state = state.clone();
    let resource_host = host.clone();
    module
        .async_function("openResource", move |args| {
            resource_host.check()?;
            if args.string(0)? != "git" {
                return Err(HostError::new("resource kind is unavailable"));
            }
            let target: Target = args
                .get(1)
                .map(|value| {
                    serde_json::from_value(decode(value)?)
                        .map_err(|error| HostError::new(error.to_string()))
                })
                .transpose()?
                .unwrap_or_default();
            let allowed = gpui_shell::with_current_app(|cx| {
                resource_owner.read_with(cx, |panel, cx| {
                    panel.selected.as_ref() == Some(&resource_host.context().package)
                        && panel
                            .metadata
                            .read(cx)
                            .entries
                            .get(&resource_host.context().package.name)
                            .and_then(|info| info.extension.as_ref())
                            .and_then(|extension| extension.desktop.as_ref())
                            .is_some_and(|desktop| {
                                desktop.renderers.iter().any(|renderer| {
                                    renderer.resource
                                        == sailry_protocol::plugin::desktop::ResourceKind::Git
                                })
                            })
                })
            })
            .and_then(Result::ok)
            .unwrap_or(false);
            if !allowed {
                return Err(HostError::new("resource renderer is not declared"));
            }
            request(
                &resource_owner,
                resource_state.clone(),
                resource_host.clone(),
                None,
                Operation::OpenResource(target.path),
            )
        })
        .async_function("openFile", move |args| {
            let path = args.string(0)?.to_owned();
            let line = args
                .get(1)
                .map(|value| {
                    serde_json::from_value::<usize>(decode(value)?)
                        .map_err(|error| HostError::new(error.to_string()))
                })
                .transpose()?;
            request(
                &file_owner,
                file_state.clone(),
                file_host.clone(),
                None,
                Operation::OpenFile(path, line),
            )
        })
        .async_function("openProjectCreation", move |args| {
            let options: ProjectCreation = serde_json::from_value(decode(args.value(0)?)?)
                .map_err(|error| HostError::new(error.to_string()))?;
            request(
                &owner,
                state.clone(),
                host.clone(),
                None,
                Operation::CreateProject(options.clone),
            )
        })
        .declarations(declarations)
}

impl Shell {
    pub(super) fn open_plugin_file(
        &mut self,
        binding: Binding,
        source: Option<WeakEntity<Chat>>,
        path: String,
        line: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, String> {
        let worktree = binding
            .worktree
            .ok_or("file navigation requires a worktree")?;
        let node = binding.client.target();
        if self
            .live
            .as_ref()
            .is_none_or(|live| live.selected != node || !live.view.connected)
        {
            return Err("file navigation context changed".into());
        }
        if let Some(source) = source.and_then(|source| source.upgrade()) {
            self.focus_chat_pane(&source, window, cx);
            if self.current_chat() != Some(&source) {
                return Err("conversation context changed".into());
            }
            self.open_documents((node, worktree), Some((path, line)), window, cx);
            return Ok(true);
        }
        let next = Rc::new(
            move |shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>| {
                let Some(live) = shell
                    .live
                    .as_mut()
                    .filter(|live| live.selected == node && live.view.connected)
                else {
                    return;
                };
                if let Some(project) = binding.project {
                    live.select_project_worktree(project, worktree);
                }
                let Some(entry) = shell
                    .renderer_navigation(
                        sailry_protocol::plugin::desktop::ResourceKind::Documents,
                        cx,
                    )
                    .filter(|entry| entry.node == node && entry.worktree == Some(worktree))
                else {
                    return;
                };
                shell.open_extension(entry, window, cx);
                let Some(panel) = shell
                    .extensions
                    .as_ref()
                    .and_then(|state| state.panel.clone())
                else {
                    return;
                };
                if panel.read(cx).resource_active() {
                    Self::reveal_document(&panel, path.clone(), line, window, cx);
                } else {
                    panel.update(cx, |panel, _| {
                        panel.pending_document = Some((path.clone(), line))
                    });
                }
            },
        );
        if !self.guard_document_navigation(window, cx, next.clone()) {
            next(self, window, cx);
        }
        Ok(true)
    }
}
