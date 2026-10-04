use super::*;
use crate::store::{Ingress, Job, unavailable};
use sailry_link::Admission;
use sailry_protocol::{Receipt, Request, VERSION};
use tokio::sync::oneshot;

pub(in crate::store) fn current(
    db: &Connection,
    package: &plugin::Reference,
    surface: plugin::desktop::Surface,
) -> Result<Info, Fault> {
    let info = required(db, &package.name)?;
    if !info.summary.enabled && surface != plugin::desktop::Surface::Settings {
        return Err(Fault::new(ErrorCode::NotConfigured, "plugin is disabled"));
    }
    if info.summary.reference() != *package {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "plugin version changed",
        ));
    }
    if info
        .extension
        .as_ref()
        .and_then(|extension| extension.desktop.as_ref())
        .is_none()
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no desktop view",
        ));
    }
    if surface == plugin::desktop::Surface::Settings
        && info
            .extension
            .as_ref()
            .and_then(|extension| extension.settings_page.as_ref())
            .and_then(|page| page.entry.as_ref())
            .is_none()
    {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no settings view",
        ));
    }
    if matches!(
        surface,
        plugin::desktop::Surface::Composer | plugin::desktop::Surface::Project
    ) && !info.extension.as_ref().is_some_and(|extension| {
        extension
            .ui
            .iter()
            .any(|entry| entry.slot.is_project() == (surface == plugin::desktop::Surface::Project))
            && extension
                .desktop
                .as_ref()
                .is_some_and(|desktop| desktop.ui_entry.is_some())
    }) {
        return Err(Fault::new(
            ErrorCode::NotConfigured,
            "plugin has no contributions for this surface",
        ));
    }
    Ok(info)
}

impl Ingress {
    pub(in crate::store) async fn read_plugin_view(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let Command::ReadPluginView { package, surface } = &request.command else {
            return Err(Fault::new(
                ErrorCode::Internal,
                "plugin view command expected",
            ));
        };
        let info = self.view_info(package.clone(), *surface).await?;
        let bundle = self
            .plugins
            .read_view(info, *surface, self.closed.clone())
            .await?;
        // Do not return a view disabled or replaced while its files were being read.
        self.view_info(package.clone(), *surface).await?;
        if self.closed.is_cancelled() {
            return Err(unavailable());
        }
        let (send, completion) = oneshot::channel();
        let _ = send.send(Ok(Output::PluginView(bundle)));
        Ok(Admission {
            receipt: Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }

    async fn view_info(
        &self,
        package: plugin::Reference,
        surface: plugin::desktop::Surface,
    ) -> Result<Info, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .try_send(Job::PluginView {
                package,
                surface,
                reply,
            })
            .map_err(|_| Fault::new(ErrorCode::Busy, "Node request queue is unavailable"))?;
        response.await.map_err(|_| unavailable())?
    }
}
