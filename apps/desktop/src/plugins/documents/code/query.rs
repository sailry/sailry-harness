use super::*;
use gpui_kit::component::input::RopeExt;
use ropey::Rope;

struct Pending {
    binding: Binding,
    request: RequestId,
    stop: CancellationToken,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.stop.cancel();
        let client = self.binding.client.clone();
        let id = self.request;
        self.binding.runtime.spawn(async move {
            let _ = client
                .execute(client.prepare(Command::CancelCodeQuery { request: id }))
                .await;
        });
    }
}

impl Provider {
    pub(super) fn request(
        &self,
        kind: Kind,
        text: &Rope,
        offset: usize,
        cx: &mut App,
    ) -> Task<anyhow::Result<code::Result>> {
        let text = text.to_string();
        let position = ropey::Rope::from(text.as_str()).offset_to_position(offset);
        let position = code::Position {
            line: position.line + 1,
            column: position.character + 1,
        };
        let stop = self.stop.child_token();
        if let Some(previous) = self.requests.borrow_mut().insert(kind, stop.clone()) {
            previous.cancel();
        }
        let controller = self.controller.clone();
        let document = self.document;
        let binding = self.binding.clone();
        let input = self.input.clone();
        cx.spawn(async move |cx| {
            let capture = cx.update(|cx| {
                controller
                    .read_with(cx, |controller, cx| {
                        let document = controller.documents.get(&document)?;
                        if document.dismissed
                            || document.truncated
                            || document.uncertain
                            || document.input.read(cx).value().as_ref() != text
                        {
                            return None;
                        }
                        Some((document.path.clone(), document.revision.clone()?))
                    })
                    .ok()
                    .flatten()
            });
            let Some((path, revision)) = capture else {
                return Err(anyhow::anyhow!("code document is unavailable"));
            };
            let query = match kind {
                Kind::Hover => code::Query::HoverAt {
                    path: path.clone(),
                    position,
                },
                Kind::Definition => code::Query::DefinitionAt {
                    path: path.clone(),
                    position,
                },
                Kind::Completion => code::Query::Completion {
                    path: path.clone(),
                    position,
                },
                Kind::Diagnostics => code::Query::Diagnostics {
                    path: path.clone(),
                    severity: None,
                },
            };
            let current = text.clone();
            let expected = blake3::hash(text.as_bytes()).to_hex().to_string();
            let request = binding.client.prepare(Command::ReadCode {
                worktree: binding.worktree.expect("code worktree"),
                query,
                overlay: Some(code::Overlay {
                    path,
                    expected_revision: revision,
                    text,
                }),
            });
            let pending = Pending {
                binding: binding.clone(),
                request: request.id,
                stop: stop.clone(),
            };
            let client = binding.client.clone();
            let job = binding.runtime.spawn(async move {
                let work = async {
                    if kind == Kind::Completion { tokio::time::sleep(Duration::from_millis(150)).await; }
                    let Output::Code(result) = client.execute(request).await? else {
                        return Err(Fault::new(ErrorCode::Internal, "code query response expected"));
                    };
                    Ok(result)
                };
                tokio::select! {
                    biased;
                    _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "code query was cancelled")),
                    result = work => result,
                }
            });
            let result = job.await??;
            let matches = input
                .read_with(cx, |input, _| input.value().as_ref() == current)
                .unwrap_or(false);
            if !matches
                || pending.stop.is_cancelled()
                || result
                    .revision
                    .as_ref()
                    .is_some_and(|revision| revision != &expected)
            {
                return Err(anyhow::anyhow!("code query belongs to an older document"));
            }
            drop(pending);
            Ok(result)
        })
    }

    pub(super) fn diagnostics(&self, text: String, cx: &mut Context<Controller>) {
        let request = self.request(Kind::Diagnostics, &Rope::from(text.as_str()), 0, cx);
        let input = self.input.clone();
        let id = self.document;
        let stop = self.stop.clone();
        let task = cx.spawn(async move |controller, cx| {
            let result = request.await;
            let _ = controller.update(cx, |controller, cx| {
                if stop.is_cancelled() {
                    return;
                }
                let Some(document) = controller.documents.get(&id) else {
                    return;
                };
                if document.dismissed
                    || document.uncertain
                    || document.input.read(cx).value().as_ref() != text
                {
                    return;
                }
                let Some(provider) = &document.code else {
                    return;
                };
                let Ok(result) = result else {
                    provider.startup.borrow_mut().take();
                    provider.configure(false, cx);
                    cx.notify();
                    return;
                };
                let ready = matches!(result.status, code::Status::Ready | code::Status::Indexing);
                provider.configure(ready, cx);
                let startup = (result.status == code::Status::Stopped)
                    .then_some(result.startup)
                    .flatten();
                *provider.startup.borrow_mut() = startup.clone();
                if let code::Content::Diagnostics(items) = result.content {
                    let items = mapping::diagnostics(&document.path, items);
                    let _ = input.update(cx, |input, cx| {
                        if let Some(set) = input.diagnostics_mut() {
                            set.clear();
                            set.extend(items);
                        }
                        cx.notify();
                    });
                }
                if let Some(startup) = startup {
                    controller.auto_code(id, startup, result.startup_approved, cx);
                }
                cx.notify();
            });
        });
        *self.diagnostics.borrow_mut() = Some(task);
    }
}

pub(super) async fn automatic(
    client: &sailry_client::Client,
    session: Option<SessionId>,
) -> Result<bool, Fault> {
    let permission = if let Some(session) = session {
        let Output::Session(session) = client
            .execute(client.prepare(Command::ReadSession { session }))
            .await?
        else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "session expected for code startup",
            ));
        };
        Some(session.config.permission)
    } else {
        let Output::Snapshot(snapshot) = client.execute(client.prepare(Command::Snapshot)).await?
        else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "Node defaults expected for code startup",
            ));
        };
        snapshot.defaults.config.map(|config| config.permission)
    };
    Ok(matches!(
        permission,
        Some(Permission::Project | Permission::Full)
    ))
}
