//! Packages choose checkout UI; the core retains conversation and resource navigation.
use super::{
    Panel,
    host::{
        Host,
        sdk::values::{decode, encode},
    },
};
use crate::{
    conversation::live::{Binding, View as Chat},
    shell::Shell,
};
use gpui_kit::*;
use gpui_shell::{HostError, HostModule};
use sailry_protocol::{
    Command, Output, Request, RequestId, RequestOutcome, Snapshot, Worktree, WorktreeId,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
};

type Reply = tokio::sync::oneshot::Sender<Result<Value, String>>;
mod navigation;
pub(super) struct Scope {
    pub binding: Binding,
    pub source: Option<WeakEntity<Chat>>,
    pub snapshot: Option<Snapshot>,
    pub connected: bool,
    pub read: bool,
    pub write: bool,
    pub session_control: bool,
}
pub(super) struct State {
    binding: Binding,
    source: Option<WeakEntity<Chat>>,
    snapshot: Option<Snapshot>,
    connected: bool,
    cursor: u64,
    changes: tokio::sync::watch::Sender<Value>,
    known: BTreeSet<WorktreeId>,
    confirmed: BTreeMap<WorktreeId, Worktree>,
    read: bool,
    write: bool,
    session_control: bool,
    _observers: Vec<Subscription>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    worktree: Option<WorktreeId>,
    #[serde(default)]
    fork: bool,
    #[serde(default)]
    projects: bool,
    #[serde(default)]
    overview: bool,
    request: Option<RequestId>,
}
#[derive(Clone)]
enum Operation {
    OpenResource(Option<String>),
    OpenFile(String, Option<usize>),
    CreateProject(bool),
    Confirm(Request, Box<Operation>),
    Invoke(sailry_protocol::plugin::ui::Intent, Option<Value>),
    Select(Selection),
    Drafts(WorktreeId),
    Release(WorktreeId, Request),
}
struct Event {
    state: Entity<State>,
    host: Arc<Host>,
    cursor: Option<String>,
    operation: Operation,
    reply: RefCell<Option<Reply>>,
}
impl EventEmitter<Event> for Panel {}

impl State {
    fn refresh(&mut self, cx: &App) {
        let mut value = if let Some(source) = self.source.as_ref().and_then(WeakEntity::upgrade) {
            source.read(cx).location_state()
        } else {
            let main = self
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .worktrees
                        .iter()
                        .find(|tree| Some(tree.id) == self.binding.worktree)
                })
                .is_none_or(|tree| tree.main);
            json!({"surface":"project","node":crate::live::node_key(self.binding.client.target()),"project":self.binding.project,"worktree":self.binding.worktree,
                "session":null,"revision":null,"main":main,"project_name":self.binding.project_name.as_ref(),"branch":self.binding.branch.as_ref(),
                "can_move":self.connected,"can_retarget":false,"git":false,"connected":self.connected})
        };
        if let Some(snapshot) = &self.snapshot {
            self.known.extend(
                snapshot
                    .worktrees
                    .iter()
                    .filter(|tree| {
                        Some(tree.id) == self.binding.worktree
                            || self.binding.project.is_some()
                                && tree.project == self.binding.project
                    })
                    .map(|tree| tree.id),
            );
        }
        value["cursor"] = self.cursor.to_string().into();
        if *self.changes.borrow() != value {
            self.cursor += 1;
            value["cursor"] = self.cursor.to_string().into();
            self.changes.send_replace(value);
        }
    }
    fn check(&self, host: &Host, cursor: Option<&str>) -> Result<(), String> {
        host.check()
            .map_err(|_| "plugin view is unavailable".to_owned())?;
        if !self.read
            || host.context().worktree != self.binding.worktree
            || self.changes.borrow()["node"]
                != json!(crate::live::node_key(self.binding.client.target()))
            || self.changes.borrow()["worktree"] != json!(self.binding.worktree)
            || self.changes.borrow()["session"] != json!(host.context().session)
        {
            return Err("location is outside the captured view".into());
        }
        if cursor.is_some_and(|cursor| cursor != self.cursor.to_string()) {
            return Err("location changed".into());
        }
        Ok(())
    }
}

pub(super) fn extend(
    module: HostModule,
    scope: Scope,
    owner: WeakEntity<Panel>,
    host: Arc<Host>,
    window: &mut Window,
    cx: &mut App,
) -> (HostModule, Entity<State>) {
    let state = cx.new(|cx| {
        let mut observers = Vec::new();
        if let Some(source) = scope.source.as_ref().and_then(WeakEntity::upgrade) {
            observers.push(cx.observe(&source, |state: &mut State, _, cx| state.refresh(cx)));
        }
        if let Some(panel) = owner.upgrade() {
            observers.push(cx.observe(&panel, |state: &mut State, panel, cx| {
                state.snapshot = panel.read(cx).snapshot.borrow().clone();
                state.connected = panel.read(cx).connected;
                state.refresh(cx);
            }));
        }
        let mut state = State {
            binding: scope.binding,
            source: scope.source,
            snapshot: scope.snapshot,
            connected: scope.connected,
            cursor: 0,
            changes: tokio::sync::watch::channel(Value::Null).0,
            known: BTreeSet::new(),
            confirmed: BTreeMap::new(),
            read: scope.read,
            write: scope.write,
            session_control: scope.session_control,
            _observers: observers,
        };
        state.refresh(cx);
        state
    });
    let invoke = state.clone();
    let available = state.clone();
    let invoke_host = host.clone();
    let available_host = host.clone();
    let invoke_owner = owner.clone();
    let available_owner = owner.clone();
    let read = state.clone();
    let changes = state.clone();
    let select = state.clone();
    let drafts = state.clone();
    let release = state.clone();
    let read_host = host.clone();
    let change_host = host.clone();
    let select_host = host.clone();
    let draft_host = host.clone();
    let release_host = host.clone();
    let select_owner = owner.clone();
    let draft_owner = owner.clone();
    let route_owner = owner.clone();
    let route_host = host.clone();
    let declarations = format!(
        "{}\nexport type ContributionIntent = 'git_branches'|'worktrees'|'create_worktree'|'fork_worktree';\nexport function invokeContribution(intent:ContributionIntent,value?:unknown):Promise<boolean>;\nexport function canInvokeContribution(intent:ContributionIntent):Promise<boolean>;\nexport function readLocation():{{cursor:string;surface:'composer'|'project';node:string;project:string|null;worktree:string|null;session:string|null;revision:string|null;main:boolean;project_name:string;branch:string;can_move:boolean;can_retarget:boolean;git:boolean;connected:boolean}};\nexport function nextLocationChange(cursor:string):Promise<ReturnType<typeof readLocation>>;\nexport function selectLocation(cursor:string,selection:{{worktree?:string;fork?:boolean;projects?:boolean;overview?:boolean;request?:string}}):Promise<boolean>;\nexport function worktreeHasDrafts(worktree:string,request?:string):Promise<boolean>;\nexport function releaseWorktree(worktree:string,request:string):Promise<void>;",
        module.declared().unwrap_or_default()
    );
    let module=module.async_function("invokeContribution",move|args|{
        let intent = serde_json::from_value(Value::String(args.string(0)?.to_owned())).map_err(|error|HostError::new(error.to_string()))?;
        let value = args.get(1).map(decode).transpose()?.unwrap_or(Value::Null);
        request(&invoke_owner,invoke.clone(),invoke_host.clone(),None,Operation::Invoke(intent,Some(value)))
    }).async_function("canInvokeContribution",move|args|{
        let intent = serde_json::from_value(Value::String(args.string(0)?.to_owned())).map_err(|error|HostError::new(error.to_string()))?;
        request(&available_owner,available.clone(),available_host.clone(),None,Operation::Invoke(intent,None))
    }).function("readLocation",move|_|{
        read_host.check()?;
        gpui_shell::with_current_app(|cx| {let state=read.read(cx);state.check(&read_host,None).map_err(HostError::new)?;encode(state.changes.borrow().clone())}).ok_or_else(||HostError::new("location requires an active view"))?
    }).async_function("nextLocationChange",move|args|{
        change_host.check()?;let seen=args.string(0)?.to_owned();
        let mut changes=gpui_shell::with_current_app(|cx| {let state=changes.read(cx);if !state.read{return Err(HostError::new("location read is unavailable"));}Ok(state.changes.subscribe())}).ok_or_else(||HostError::new("location requires an active view"))??;
        let stop=change_host.stop_token();Ok(async move {loop {let value=changes.borrow_and_update().clone();if value["cursor"].as_str()!=Some(&seen){return encode(value);}tokio::select!{biased;_=stop.cancelled()=>return Err(HostError::new("plugin view is closed")),result=changes.changed()=>result.map_err(|_|HostError::new("location is closed"))?}}})
    }).async_function("selectLocation",move|args|{
        let cursor=args.string(0)?.to_owned();let selection:Selection=serde_json::from_value(decode(args.value(1)?)?).map_err(|error|HostError::new(error.to_string()))?;
        let prepared=selection.request.map(|id|select_host.prepared(id)).transpose()?;
        let operation=Operation::Select(selection);
        let operation=prepared.map_or_else(||operation.clone(),|request|Operation::Confirm(request,Box::new(operation.clone())));
        request(&select_owner,select.clone(),select_host.clone(),Some(cursor),operation)
    }).async_function("worktreeHasDrafts",move|args|{
        let worktree=args.string(0)?.parse().map_err(|_|HostError::new("invalid worktree ID"))?;
        let operation=Operation::Drafts(worktree);
        let receipt=args.get(1).map(|value|serde_json::from_value::<Option<RequestId>>(decode(value)?).map_err(|error|HostError::new(error.to_string()))).transpose()?.flatten();
        let operation=if let Some(id)=receipt {Operation::Confirm(draft_host.prepared(id)?,Box::new(operation))}else{operation};
        request(&draft_owner,drafts.clone(),draft_host.clone(),None,operation)
    }).async_function("releaseWorktree",move|args|{
        let worktree=args.string(0)?.parse().map_err(|_|HostError::new("invalid worktree ID"))?;
        let id=args.string(1)?.parse::<RequestId>().map_err(|_|HostError::new("invalid request ID"))?;
        let prepared=release_host.prepared(id)?;
        request(&owner,release.clone(),release_host.clone(),None,Operation::Release(worktree,prepared))
    }).declarations(declarations);
    let _ = window;
    (
        navigation::module(module, state.clone(), route_owner, route_host),
        state,
    )
}

fn request(
    owner: &WeakEntity<Panel>,
    state: Entity<State>,
    host: Arc<Host>,
    cursor: Option<String>,
    operation: Operation,
) -> Result<
    impl Future<Output = Result<gpui_shell::HostValue, HostError>> + Send + 'static + use<>,
    HostError,
> {
    host.check()?;
    let owner = owner.clone();
    let (reply, receive) = tokio::sync::oneshot::channel();
    gpui_shell::with_current_app(|cx| {
        cx.defer(move |cx| {
            let _ = owner.update(cx, |_, cx| {
                cx.emit(Event {
                    state,
                    host,
                    cursor,
                    operation,
                    reply: RefCell::new(Some(reply)),
                })
            });
        })
    })
    .ok_or_else(|| HostError::new("location requires an active view"))?;
    Ok(async move {
        receive
            .await
            .map_err(|_| HostError::new("location is closed"))?
            .map_err(HostError::new)
            .and_then(encode)
    })
}

impl Shell {
    pub(super) fn observe_plugin_locations(
        panel: &Entity<Panel>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(panel, window, |shell, _, event: &Event, window, cx| {
            let Some(reply) = event.reply.borrow_mut().take() else {
                return;
            };
            shell.apply_plugin_location(event, reply, window, cx);
        })
        .detach();
    }

    fn apply_plugin_location(
        &mut self,
        event: &Event,
        reply: Reply,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shell = self;
        let state = event.state.read(cx);
        if let Err(error) = state.check(&event.host, event.cursor.as_deref()) {
            let _ = reply.send(Err(error));
            return;
        }
        let node = state.binding.client.target();
        match &event.operation {
            Operation::OpenResource(path) => {
                let source = state.source.as_ref().and_then(WeakEntity::upgrade);
                if let Some(tree) = state.binding.worktree
                    && let Some(source) = source
                    && shell
                        .live
                        .as_ref()
                        .is_some_and(|live| live.selected == node && live.view.connected)
                {
                    shell.focus_chat_pane(&source, window, cx);
                    shell.open_git_resource((node, tree), Some(path.clone()), window, cx);
                    let _ = reply.send(Ok(Value::Bool(true)));
                } else {
                    let _ = reply.send(Err("resource context changed".into()));
                }
            }
            Operation::OpenFile(path, line) => {
                let binding = state.binding.clone();
                let source = state.source.clone();
                let result =
                    shell.open_plugin_file(binding, source, path.clone(), *line, window, cx);
                let _ = reply.send(result.map(Value::Bool));
            }
            Operation::CreateProject(clone) => {
                let result = if shell
                    .live
                    .as_ref()
                    .is_some_and(|live| live.selected == node && live.view.connected)
                {
                    if *clone {
                        shell.clone_git_project(window, cx);
                    } else {
                        shell.project_editor(0, None, window, cx);
                    }
                    Ok(Value::Bool(true))
                } else {
                    Err("project context changed".into())
                };
                let _ = reply.send(result);
            }
            Operation::Confirm(request, operation) => {
                let expected = match operation.as_ref() {
                    Operation::Select(selection) => selection.worktree,
                    Operation::Drafts(tree) => Some(*tree),
                    _ => None,
                };
                let project = state.binding.project;
                let admitted = match &request.command {
                    Command::RegisterWorktree {
                        project: target, ..
                    }
                    | Command::CreateWorktree {
                        project: target, ..
                    } => Some(*target) == project,
                    Command::CreateManagedWorktree {
                        project: target,
                        source,
                        ..
                    } => Some(*target) == project && Some(*source) == state.binding.worktree,
                    _ => false,
                };
                if !admitted || expected.is_none() {
                    let _ = reply.send(Err(
                        "worktree handoff is outside the captured project".into()
                    ));
                    return;
                }
                let client = state.binding.client.clone();
                let request = request.clone();
                let job = state
                    .binding
                    .runtime
                    .spawn(async move { client.outcome(&request).await });
                let next = Event {
                    state: event.state.clone(),
                    host: event.host.clone(),
                    cursor: event.cursor.clone(),
                    operation: *operation.clone(),
                    reply: RefCell::new(None),
                };
                cx.spawn_in(window, async move |shell, cx| {
                    let result = job.await;
                    let tree = match result {
                        Ok(Ok(RequestOutcome::Completed(result))) => match *result {
                            Ok(Output::Worktree(tree))
                                if Some(tree.id) == expected && tree.project == project =>
                            {
                                Some(tree)
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some(tree) = tree else {
                        let _ = reply.send(Err("worktree handoff is not confirmed".into()));
                        return;
                    };
                    let _ = shell.update_in(cx, |shell, window, cx| {
                        next.state.update(cx, |state, _| {
                            state.known.insert(tree.id);
                            state.confirmed.insert(tree.id, tree);
                        });
                        shell.apply_plugin_location(&next, reply, window, cx);
                    });
                })
                .detach();
            }
            Operation::Invoke(intent, value) => {
                let task = shell.plugin_intent(
                    state.binding.clone(),
                    state.source.clone(),
                    *intent,
                    value.clone(),
                    window,
                    cx,
                );
                cx.spawn(async move |_, _| {
                    let _ = reply.send(task.await);
                })
                .detach();
            }
            Operation::Drafts(tree) => {
                let result = if state.known.contains(tree) {
                    Ok(json!(shell.worktree_has_drafts((node, *tree), cx)))
                } else {
                    Err("worktree is outside the captured project".into())
                };
                let _ = reply.send(result);
            }
            Operation::Release(tree, request) => {
                if !state.write
                    || !matches!(&request.command,Command::RemoveWorktree{worktree,..} if worktree==tree)
                {
                    let _ = reply.send(Err(
                        "worktree removal is outside the captured project".into()
                    ));
                    return;
                }
                let scope = (node, *tree);
                let client = state.binding.client.clone();
                let request = request.clone();
                let job = state
                    .binding
                    .runtime
                    .spawn(async move { client.outcome(&request).await });
                cx.spawn_in(window,async move|shell,cx| {let result=job.await;
                        let confirmed=matches!(result,Ok(Ok(RequestOutcome::Completed(result))) if matches!(*result,Ok(Output::WorktreeRemoved{id}) if id==scope.1));
                        let result=if confirmed {shell.update_in(cx,|shell,window,cx|{shell.release_worktree(scope,window,cx);Value::Null}).map_err(|_|"window is closed".to_owned())} else{Err("worktree removal is not confirmed".into())};let _=reply.send(result);
                    }).detach();
            }
            Operation::Select(selection) => {
                if usize::from(selection.worktree.is_some())
                    + usize::from(selection.projects)
                    + usize::from(selection.overview)
                    != 1
                    || selection
                        .worktree
                        .is_some_and(|tree| !state.known.contains(&tree))
                {
                    let _ = reply.send(Err("invalid location selection".into()));
                    return;
                }
                if let Some(source) = state.source.as_ref().and_then(WeakEntity::upgrade) {
                    if !state.session_control || selection.overview {
                        let _ =
                            reply.send(Err("conversation location control is unavailable".into()));
                        return;
                    }
                    let confirmed = selection
                        .worktree
                        .and_then(|tree| state.confirmed.get(&tree))
                        .cloned();
                    let result = source
                        .update(cx, |source, cx| {
                            source.select_registered_location(
                                selection.worktree,
                                selection.fork,
                                confirmed.as_ref(),
                                window,
                                cx,
                            )
                        })
                        .map(|_| Value::Bool(true));
                    let _ = reply.send(result);
                } else {
                    if selection.projects
                        || selection.fork
                        || shell.live.as_ref().is_none_or(|live| live.selected != node)
                    {
                        let _ = reply.send(Err("project location is unavailable".into()));
                        return;
                    }
                    let Some(project) = state.binding.project else {
                        let _ = reply.send(Err("location requires a project".into()));
                        return;
                    };
                    let tree = selection.worktree;
                    let reply = Rc::new(RefCell::new(Some(reply)));
                    let next = Rc::new(
                        move |shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>| {
                            if let Some(live) =
                                shell.live.as_mut().filter(|live| live.selected == node)
                            {
                                if let Some(tree) = tree {
                                    live.select_project_worktree(project, tree);
                                }
                                shell.live_resource_action(
                                    &crate::live::menus::Dispatch {
                                        node,
                                        target: crate::live::menus::Target::Project(project),
                                        command: crate::live::menus::Command::Open,
                                    },
                                    window,
                                    cx,
                                );
                                if let Some(reply) = reply.borrow_mut().take() {
                                    let _ = reply.send(Ok(Value::Bool(true)));
                                }
                            }
                            let _ = window;
                        },
                    );
                    if !shell.guard_document_navigation(window, cx, next.clone()) {
                        next(shell, window, cx);
                    }
                }
            }
        }
    }
}
