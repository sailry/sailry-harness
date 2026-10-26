use super::{Inbox, Target};
use crate::Client;
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, NotificationId, Output, Request, RequestId,
};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Change {
    ReadAll,
    DismissRead,
}

/// An explicit controller action. Unconfirmed requests stay in the Inbox so
/// another explicit attempt can reuse their captured content and identifiers.
#[derive(Clone)]
pub struct Batch {
    change: Change,
    requests: Vec<Request>,
    local: usize,
}

#[derive(Default)]
pub struct Report {
    pub completed: usize,
    pub errors: Vec<(NodeId, Fault)>,
    confirmed: Vec<RequestId>,
    settled: Vec<RequestId>,
}

impl Inbox {
    pub fn prepare(&mut self, change: Change) -> Batch {
        let pending = self.pending.entry(change).or_default();
        let mut nodes = BTreeMap::<NodeId, Vec<NotificationId>>::new();
        let mut local = 0;
        for notice in &mut self.notices {
            if notice.read != (change == Change::DismissRead) {
                continue;
            }
            if let Target::Notification(id) = notice.id.target {
                if !pending
                    .iter()
                    .any(|request| request.target == notice.id.node && ids(request).contains(&id))
                {
                    nodes.entry(notice.id.node).or_default().push(id);
                }
            } else if change == Change::ReadAll {
                notice.read = true;
                local += 1;
            }
        }
        for (node, ids) in nodes {
            pending.push(Request::new(
                node,
                match change {
                    Change::ReadAll => Command::MarkNotificationsRead { ids },
                    Change::DismissRead => Command::DismissReadNotifications { ids },
                },
            ));
        }
        Batch {
            change,
            requests: pending.clone(),
            local,
        }
    }

    pub fn complete(&mut self, batch: &Batch, report: &Report) {
        let confirmed: Vec<_> = batch
            .requests
            .iter()
            .filter(|request| report.confirmed.contains(&request.id))
            .collect();
        self.notices.retain_mut(|notice| {
            let Target::Notification(id) = notice.id.target else {
                return true;
            };
            if confirmed
                .iter()
                .any(|request| request.target == notice.id.node && ids(request).contains(&id))
            {
                if batch.change == Change::DismissRead {
                    return false;
                }
                notice.read = true;
            }
            true
        });
        if let Some(pending) = self.pending.get_mut(&batch.change) {
            pending.retain(|request| !report.settled.contains(&request.id));
        }
    }
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.requests.is_empty() && self.local == 0
    }

    pub fn nodes(&self) -> impl Iterator<Item = NodeId> {
        self.requests
            .iter()
            .map(|request| request.target)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
    }

    pub async fn run(&self, clients: &BTreeMap<NodeId, Arc<Client>>) -> Report {
        let mut report = Report {
            completed: self.local,
            ..Default::default()
        };
        for request in &self.requests {
            let Some(client) = clients.get(&request.target) else {
                report.errors.push((
                    request.target,
                    Fault::new(ErrorCode::Unavailable, "notification Node is unavailable"),
                ));
                continue;
            };
            let result = client.execute(request.clone()).await;
            if !matches!(&result, Err(error) if error.code == ErrorCode::OutcomeUnknown) {
                report.settled.push(request.id);
            }
            match result {
                Ok(Output::NotificationsUpdated) => {
                    report.completed += 1;
                    report.confirmed.push(request.id);
                }
                Err(error) => report.errors.push((request.target, error)),
                _ => report.errors.push((
                    request.target,
                    Fault::new(ErrorCode::Internal, "notification update response expected"),
                )),
            }
        }
        report
    }
}

impl Report {
    pub fn failed(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn unknown(&self) -> bool {
        self.errors
            .iter()
            .any(|(_, error)| error.code == ErrorCode::OutcomeUnknown)
    }

    /// Keep the source error, including an uncertain outcome, available to thin
    /// adapters whose public method can return only one diagnostic.
    pub fn fault(&self) -> Option<Fault> {
        let (_, error) = self
            .errors
            .iter()
            .find(|(_, error)| error.code == ErrorCode::OutcomeUnknown)
            .or_else(|| self.errors.first())?;
        let mut error = error.clone();
        if self.completed > 0 {
            error.message = format!("some notifications could not be updated: {}", error.message);
        }
        Some(error)
    }
}

fn ids(request: &Request) -> &[NotificationId] {
    match &request.command {
        Command::MarkNotificationsRead { ids } | Command::DismissReadNotifications { ids } => ids,
        _ => unreachable!("notification batches capture only notification commands"),
    }
}
