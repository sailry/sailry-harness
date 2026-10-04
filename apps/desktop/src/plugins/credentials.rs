//! Opaque controller drafts keep credential contents outside script values.
//! Native Kit inputs retain editing, selection, accessibility and masked display.
use gpui_kit::{
    component::input::{Input, InputEvent, InputState, Textarea, TextareaState},
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use sailry_protocol::Secret;
use std::{collections::BTreeMap, io::Read, sync::Arc};

pub(in crate::plugins) mod loading;

pub(in crate::plugins) struct Store {
    sequence: u64,
    drafts: BTreeMap<String, Draft>,
    events: tokio::sync::mpsc::Sender<serde_json::Value>,
    receiver: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<serde_json::Value>>>,
}
impl Default for Store {
    fn default() -> Self {
        let (events, receiver) = tokio::sync::mpsc::channel(32);
        Self {
            sequence: 0,
            drafts: BTreeMap::new(),
            events,
            receiver: Arc::new(tokio::sync::Mutex::new(receiver)),
        }
    }
}
struct Draft {
    label: String,
    placeholder: String,
    multiline: bool,
    input: Option<Text>,
    imported: Option<(String, Secret)>,
    picking: bool,
    pending: Option<Secret>,
    version: u64,
    loading: u64,
    subscription: Option<Subscription>,
}
enum Text {
    Input(Entity<InputState>),
    Area(Entity<TextareaState>),
}

impl Draft {
    fn value(&self, cx: &App) -> Secret {
        if let Some(secret) = &self.pending {
            return secret.clone();
        }
        if let Some((_, secret)) = &self.imported {
            return secret.clone();
        }
        Secret::new(match &self.input {
            Some(Text::Input(input)) => input.read(cx).value().to_string(),
            Some(Text::Area(input)) => input.read(cx).value().to_string(),
            None => String::new(),
        })
    }
}
impl Store {
    pub(in crate::plugins) fn read(&self, id: &str, cx: &App) -> Result<Secret, HostError> {
        let draft = self.drafts.get(id).ok_or_else(missing)?;
        if draft.picking {
            return Err(HostError::new("credential file selection is pending"));
        }
        Ok(draft.value(cx))
    }
    fn observe(&mut self, id: &str, event: &InputEvent, cx: &mut Context<Self>) {
        let Some(draft) = self.drafts.get_mut(id) else {
            return;
        };
        let kind = match event {
            InputEvent::Change => {
                // User input wins over a completed read awaiting the next frame.
                draft.pending = None;
                draft.version += 1;
                "change"
            }
            InputEvent::Blur => "blur",
            InputEvent::PressEnter { .. } => "enter",
            InputEvent::Focus => "focus",
        };
        let _ = self.events.try_send(serde_json::json!({
            "id": id, "kind": kind, "version": draft.version,
            "filled": !draft.value(cx).expose().is_empty(),
        }));
        cx.notify();
    }
    fn choose(
        &mut self,
        id: String,
        prompt: String,
        reply: tokio::sync::oneshot::Sender<Result<Option<String>, String>>,
        cx: &mut Context<Self>,
    ) {
        let Some(draft) = self.drafts.get_mut(&id) else {
            let _ = reply.send(Err("credential draft is unavailable".into()));
            return;
        };
        if draft.picking {
            let _ = reply.send(Err("credential file selection is pending".into()));
            return;
        }
        draft.picking = true;
        let selected = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(prompt.into()),
        });
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let result = match selected.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => {
                        executor
                            .spawn(async move {
                                let file = std::fs::File::open(&path)
                                    .map_err(|_| "credential file could not be read".to_owned())?;
                                if !file
                                    .metadata()
                                    .map_err(|_| "credential file could not be read".to_owned())?
                                    .is_file()
                                {
                                    return Err("credential file must be a regular file".into());
                                }
                                let mut text = String::new();
                                file.take(65537).read_to_string(&mut text).map_err(|_| {
                                    "credential file must contain UTF-8 text".to_owned()
                                })?;
                                if text.is_empty() || text.len() > 65536 {
                                    return Err(
                                        "credential file must contain between 1 and 65536 bytes"
                                            .into(),
                                    );
                                }
                                Ok(Some((
                                    path.file_name()
                                        .ok_or_else(|| {
                                            "credential file name is unavailable".to_owned()
                                        })?
                                        .to_string_lossy()
                                        .into_owned(),
                                    Secret::new(text),
                                )))
                            })
                            .await
                    }
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                _ => Err("credential file selection failed".into()),
            };
            let result = this
                .update(cx, |this, cx| {
                    let draft = this
                        .drafts
                        .get_mut(&id)
                        .ok_or_else(|| "credential draft is unavailable".to_owned())?;
                    draft.picking = false;
                    let result = result.map(|file| {
                        let name = file.as_ref().map(|(name, _)| name.clone());
                        if let Some(file) = file {
                            draft.imported = Some(file);
                        }
                        name
                    });
                    cx.notify();
                    result
                })
                .unwrap_or_else(|_| Err("credential draft is unavailable".into()));
            let _ = reply.send(result);
        })
        .detach();
    }
}

pub(in crate::plugins) fn module(fields: Entity<Store>) -> HostModule {
    let create = fields.clone();
    let release = fields.clone();
    let describe = fields.clone();
    let choose = fields.clone();
    let clear = fields.clone();
    let events = fields.clone();
    HostModule::new("sailry/credentials")
        .function("createSecret",move |args| {
            let options=args.value(0)?;
            let label=options.get("label").and_then(HostValue::as_str).unwrap_or_default().to_owned();
            let placeholder=options.get("placeholder").and_then(HostValue::as_str).unwrap_or_default().to_owned();
            let multiline=options.get("multiline").and_then(HostValue::as_bool)==Some(true);
            gpui_shell::with_current_app(|cx|create.update(cx,|fields,_| {
                if fields.drafts.len()>=16 { return Err(HostError::new("credential draft capacity exhausted")); }
                fields.sequence+=1; let id=format!("secret-{}",fields.sequence);
                fields.drafts.insert(id.clone(),Draft{label,placeholder,multiline,input:None,imported:None,picking:false,pending:None,version:0,loading:0,subscription:None});
                Ok(HostValue::from(id))
            })).ok_or_else(missing)?
        })
        .function("releaseSecret",move |args| {
            let id=args.string(0)?.to_owned();
            gpui_shell::with_current_app(|cx|release.update(cx,|fields,_|{fields.drafts.remove(&id);})).ok_or_else(missing)?;
            Ok(HostValue::Null)
        })
        .function("describeSecret",move |args| {
            let id=args.string(0)?;
            gpui_shell::with_current_app(|cx| {
                let fields=describe.read(cx); let draft=fields.drafts.get(id).ok_or_else(missing)?;
                super::host::sdk::values::encode(serde_json::json!({"filled":!draft.value(cx).expose().is_empty(),"file":draft.imported.as_ref().map(|(name,_)|name),"picking":draft.picking,"version":draft.version}))
            }).ok_or_else(missing)?
        })
        .function("clearSecret",move |args| {
            let id=args.string(0)?.to_owned();
            gpui_shell::with_current_app(|cx|clear.update(cx,|fields,cx| {
                let draft=fields.drafts.get_mut(&id).ok_or_else(missing)?;
                draft.pending=Some(Secret::new(String::new())); draft.imported=None; draft.version+=1;
                cx.notify(); Ok(HostValue::Null)
            })).ok_or_else(missing)?
        })
        .async_function("nextSecretEvent",move |_| {
            let receiver=gpui_shell::with_current_app(|cx|events.read(cx).receiver.clone()).ok_or_else(missing)?;
            Ok(async move {
                let event=receiver.lock().await.recv().await.ok_or_else(missing)?;
                super::host::sdk::values::encode(event)
            })
        })
        .async_function("chooseSecretFile",move |args| {
            let id=args.string(0)?.to_owned(); let prompt=args.string(1)?.to_owned(); let fields=choose.clone();
            let (reply,receive)=tokio::sync::oneshot::channel();
            gpui_shell::with_current_app(|cx|cx.defer(move |cx|fields.update(cx,|fields,cx|fields.choose(id,prompt,reply,cx)))).ok_or_else(missing)?;
            Ok(async move { super::host::sdk::values::encode(serde_json::json!(receive.await.map_err(|_|missing())?.map_err(HostError::new)?)) })
        })
        .component("SecretField",move |args,window,cx| {
            fields.update(cx,|fields,cx| {
                let Some(draft)=fields.drafts.get_mut(args.id()) else { return div().into_any_element(); };
                let input=draft.input.get_or_insert_with(||if draft.multiline {
                    Text::Area(cx.new(|cx|TextareaState::new(window,cx).auto_grow(3,6).placeholder(draft.placeholder.clone())))
                } else {
                    Text::Input(cx.new(|cx|InputState::new(window,cx).masked(true).placeholder(draft.placeholder.clone())))
                });
                if draft.subscription.is_none() {
                    let id=args.id().to_owned();
                    draft.subscription=Some(match input {
                        Text::Input(input)=>cx.subscribe(input,move |fields,_,event,cx|fields.observe(&id,event,cx)),
                        Text::Area(input)=>cx.subscribe(input,move |fields,_,event,cx|fields.observe(&id,event,cx)),
                    });
                }
                if let Some(secret)=draft.pending.take() {
                    match input {
                        Text::Input(input)=>input.update(cx,|input,cx|input.set_value(secret.expose().to_owned(),window,cx)),
                        Text::Area(input)=>input.update(cx,|input,cx|input.set_value(secret.expose().to_owned(),window,cx)),
                    }
                }
                let disabled=args.props().get("disabled").and_then(HostValue::as_bool)==Some(true) || draft.picking;
                let content=match input {
                    Text::Input(input)=>Input::new(input).aria_label(draft.label.clone()).mask_toggle().disabled(disabled).into_any_element(),
                    Text::Area(input)=>Textarea::new(input).aria_label(draft.label.clone()).disabled(disabled).into_any_element(),
                };
                let view=div().w_full().child(content);
                #[cfg(test)] let view=view.debug_selector({let id=args.id().to_owned();move||id.clone()});
                view.into_any_element()
            })
        }).declarations(include_str!("credentials/api.d.ts"))
}
fn missing() -> HostError {
    HostError::new("credential draft is unavailable")
}
