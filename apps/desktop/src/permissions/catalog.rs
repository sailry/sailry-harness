//! Permission contributions retain package ownership while sharing native settings rows.
use super::*;
use sailry_client::{
    Client,
    permissions::{Aggregate, Entry, aggregate, directory},
};
use sailry_protocol::{permissions::Kind, plugin::Info};
use std::sync::Arc;

impl Resource {
    pub(super) fn from_kind(kind: Kind) -> Self {
        match kind {
            Kind::FullDisk => Self::FullDisk,
            Kind::ScreenRecording => Self::Screen,
            Kind::Accessibility => Self::Accessibility,
            Kind::Microphone => Self::Microphone,
        }
    }
}

impl Panel {
    pub(crate) fn bind(
        &mut self,
        client: Arc<Client>,
        runtime: Arc<tokio::runtime::Runtime>,
        packages: &[Info],
        connected: bool,
        cx: &mut Context<Self>,
    ) {
        let sources = directory(client.target(), packages);
        let binding = Some((client.target(), connected));
        if self.sources == sources && self.binding == binding {
            return;
        }
        self.stop.cancel();
        self.stop = CancellationToken::new();
        self.task = None;
        self.checking = false;
        self.pending = None;
        self.binding = binding;
        self.sources = sources.clone();
        let local = cx
            .try_global::<crate::backend::Services>()
            .map(|services| services.local.target());
        let mut cards = app::catalog(Vec::new(), cx);
        for entry in sources {
            let resource = Resource::from_kind(entry.identity.kind);
            let owner = Some((entry.identity.device, entry.identity.subject.clone()));
            if cards
                .iter()
                .any(|card| card.resource == resource && card.owner == owner)
            {
                continue;
            }
            let mut card = if entry.declaration.check.is_none() {
                app::standard(resource)
            } else {
                Card {
                    owner: owner.clone(),
                    resource,
                    status: Status::Unknown,
                    settings: (cfg!(target_os = "macos") && local == Some(entry.identity.device))
                        .then(|| app::standard(resource).settings)
                        .flatten(),
                    check: connected
                        .then(|| action(client.clone(), runtime.clone(), entry.clone(), false)),
                    request: (connected
                        && local == Some(entry.identity.device)
                        && entry.declaration.request.is_some())
                    .then(|| action(client.clone(), runtime.clone(), entry.clone(), true)),
                    requires: None,
                }
            };
            card.owner = owner;
            if entry.declaration.check.is_none() && local != Some(entry.identity.device) {
                card.status = Status::Remote;
                card.check = None;
                card.request = None;
                card.settings = None;
            }
            cards.push(card);
        }
        for card in &mut cards {
            if let Some(previous) = self
                .cards
                .iter()
                .find(|previous| previous.owner == card.owner && previous.resource == card.resource)
                && (connected
                    || card
                        .owner
                        .as_ref()
                        .is_some_and(|(_, subject)| subject == "sailry"))
                && !matches!(card.status, Status::Remote | Status::Unavailable)
            {
                card.status = previous.status;
            }
        }
        self.cards = cards;
        self.refresh = self.active;
        cx.notify();
    }

    pub(super) fn rows(&self) -> Vec<Card> {
        rows(&self.cards)
    }
}

pub(super) fn rows(cards: &[Card]) -> Vec<Card> {
    let mut rows: Vec<Card> = Vec::new();
    for card in cards.iter().filter(|card| card.resource.is_system()) {
        if rows.iter().any(|row| row.resource == card.resource) {
            continue;
        }
        let members: Vec<_> = cards
            .iter()
            .filter(|member| member.resource == card.resource)
            .collect();
        let mut row = card.clone();
        if members.len() > 1 {
            row.status = match aggregate(
                members
                    .iter()
                    .map(|member| matches!(member.status, Status::Granted | Status::NotNeeded)),
            ) {
                Aggregate::Authorized => Status::Granted,
                Aggregate::Partial => Status::Partial,
                Aggregate::Unauthorized => Status::Required,
            };
            row.settings = members
                .iter()
                .filter(|member| !matches!(member.status, Status::Remote | Status::Unavailable))
                .find_map(|member| member.settings);
            row.check = members.iter().find_map(|member| member.check.clone());
            row.request = members
                .iter()
                .filter(|member| !matches!(member.status, Status::Granted | Status::NotNeeded))
                .find_map(|member| member.request.clone());
        }
        rows.push(row);
    }
    rows
}
fn action(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    entry: Entry,
    request: bool,
) -> Action {
    Rc::new(move |cx, stop| {
        let client = client.clone();
        let entry = entry.clone();
        let task = runtime.spawn(async move {
            let result = tokio::select! {
                biased;
                _ = stop.cancelled() => return Err(failure("permission_cancelled")),
                result = client.permission_status(&entry, request) => result,
            }
            .map_err(|_| failure("permission_failed"))?;
            Ok(result
                .grants
                .into_iter()
                .map(|grant| {
                    (
                        Resource::from_kind(grant.kind),
                        match grant.authorized {
                            Some(true) => Status::Granted,
                            Some(false) => Status::Required,
                            None => Status::Unavailable,
                        },
                    )
                })
                .collect())
        });
        cx.background_executor()
            .spawn(async move { task.await.map_err(|_| failure("permission_failed"))? })
    })
}
fn failure(key: &str) -> Failure {
    Failure {
        key: key.into(),
        status: Status::Unknown,
    }
}
