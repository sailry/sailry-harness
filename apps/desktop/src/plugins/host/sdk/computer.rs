//! Permission guidance uses the captured execution Node's public command boundary.
use super::*;
use crate::permissions::{self, Action, Card, Failure, Resource, Status};
use gpui_kit::Entity;
use sailry_protocol::computer::{Permission, Permissions};
use std::{cell::RefCell, rc::Rc};

impl Host {
    pub(super) fn computer_module(
        self: &Arc<Self>,
        module: HostModule,
        controller: Option<Entity<permissions::Controller>>,
    ) -> HostModule {
        let read = self.clone();
        let request = self.clone();
        module
            .async_function("readComputerPermissions", move |_| read.permissions(None))
            .async_function("requestComputerPermission", move |args| {
                let permission = serde_json::from_value(Value::String(args.string(0)?.into()))
                    .map_err(|_| HostError::new("invalid computer permission"))?;
                request.permission_dialog(permission, &controller)
            })
    }

    fn permission_dialog(
        self: &Arc<Self>,
        permission: Permission,
        controller: &Option<Entity<permissions::Controller>>,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        self.check()?;
        let controller = controller
            .clone()
            .ok_or_else(|| HostError::new("computer permissions require an active view"))?;
        let host = self.clone();
        let stop = self.stop_token();
        let (reply, receive) = tokio::sync::oneshot::channel();
        gpui_shell::with_current_app(|cx| {
            cx.defer(move |cx| {
                let resource = match permission {
                    Permission::ScreenCapture => Resource::Screen,
                    Permission::Accessibility => Resource::Accessibility,
                };
                let latest = Arc::new(Mutex::new(None));
                let action = |request: bool| -> Action {
                    let host = host.clone();
                    let latest = latest.clone();
                    Rc::new(move |cx, stop| {
                        let host = host.clone();
                        let latest = latest.clone();
                        cx.background_executor().spawn(async move {
                            if stop.is_cancelled() {
                                return Err(Failure {
                                    key: "permission_cancelled".into(),
                                    status: Status::Unknown,
                                });
                            }
                            let value = host
                                .permissions(request.then_some(permission))
                                .map_err(failure)?
                                .await
                                .map_err(failure)?;
                            let state: Permissions = serde_json::from_value(
                                decode(&value).map_err(failure)?,
                            )
                            .map_err(|_| Failure {
                                key: "computer_permissions_failed".into(),
                                status: Status::Unknown,
                            })?;
                            let status = if !state.local {
                                Status::Remote
                            } else if state.platform != "macos" {
                                Status::Unavailable
                            } else {
                                match permission {
                                    Permission::ScreenCapture => state.screen_capture,
                                    Permission::Accessibility => state.accessibility,
                                }
                                .map_or(
                                    Status::Unknown,
                                    |granted| {
                                        if granted {
                                            Status::Granted
                                        } else {
                                            Status::Required
                                        }
                                    },
                                )
                            };
                            *latest.lock().unwrap() = Some(value);
                            Ok(vec![(resource, status)])
                        })
                    })
                };
                let check = action(false);
                let request = action(true);
                controller.update(cx, |_, cx| {
                    cx.emit(permissions::Request {
                        cards: vec![Card {
                            resource,
                            status: Status::Unknown,
                            settings: None,
                            check: Some(check),
                            request: Some(request),
                            requires: None,
                        }],
                        stop,
                        completion: RefCell::new(Some(Box::new(move |granted, _, _| {
                            let result = if granted {
                                latest.lock().unwrap().take().ok_or_else(|| {
                                    HostError::new("computer permissions are unavailable")
                                })
                            } else {
                                Ok(HostValue::Null)
                            };
                            let _ = reply.send(result);
                        }))),
                    })
                });
            })
        })
        .ok_or_else(|| HostError::new("computer permissions require an active view"))?;
        Ok(async move {
            receive
                .await
                .map_err(|_| HostError::new("computer permissions view is closed"))?
        })
    }

    fn permissions(
        self: &Arc<Self>,
        permission: Option<Permission>,
    ) -> Result<
        impl Future<Output = Result<HostValue, HostError>> + Send + 'static + use<>,
        HostError,
    > {
        let node = self.client.target();
        let request = self.read_public(
            permission.map_or(Command::ReadComputerPermissions, |permission| {
                Command::RequestComputerPermission { permission }
            }),
        )?;
        Ok(async move {
            let value = request.await?;
            let permissions: Permissions = serde_json::from_value(decode(&value)?)
                .map_err(|_| HostError::new("invalid computer permissions response"))?;
            if permissions.node != node {
                return Err(HostError::new(
                    "computer permissions belong to another Node",
                ));
            }
            Ok(value)
        })
    }
}
fn failure(_: HostError) -> Failure {
    Failure {
        key: "computer_permissions_failed".into(),
        status: Status::Unknown,
    }
}
