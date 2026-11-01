//! Read-only catalog refresh and reachability probes have the visible draft's lifetime.
use super::*;
use sailry_protocol::{Command, Output};
use tokio::{sync::mpsc, task::JoinSet};

pub(super) struct Read {
    stop: CancellationToken,
    _task: Task<()>,
}

impl Drop for Read {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

enum Event {
    Catalog(Listing),
    Timing(Selection, Timing),
}

pub(super) fn catalog(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    selected: Option<RelaySelection>,
    revision: u64,
    window: &mut Window,
    cx: &mut Context<Picker>,
) -> Read {
    let stop = CancellationToken::new();
    let cancelled = stop.clone();
    let (sender, mut receiver) = mpsc::channel(16);
    runtime.spawn(async move {
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => {},
            _ = observe(client, selected.map(super::super::edit::desired), sender) => {},
        }
    });
    let task = cx.spawn_in(window, async move |owner, cx| {
        while let Some(event) = receiver.recv().await {
            let active = owner.update_in(cx, |picker, window, cx| {
                if picker.revision != revision {
                    return false;
                }
                match event {
                    Event::Catalog(listing) => picker.accept(listing, window, cx),
                    Event::Timing(selection, timing) => {
                        picker.timing(&selection, timing, window, cx)
                    }
                }
                true
            });
            if !matches!(active, Ok(true)) {
                break;
            }
        }
        let _ = owner.update_in(cx, |picker, window, cx| {
            if picker.revision != revision {
                return;
            }
            for item in &mut picker.items {
                if item.choice != Choice::Custom && item.timing == Timing::Pending {
                    item.timing = Timing::Measured(None);
                }
            }
            picker.sync(window, cx);
        });
    });
    Read { stop, _task: task }
}

async fn listing(client: &Client, refresh: bool) -> Option<Listing> {
    match client
        .execute(client.prepare(Command::ListNetworkPresets { refresh }))
        .await
    {
        Ok(Output::NetworkPresets(listing)) => Some(listing),
        _ => None,
    }
}

async fn measure(client: &Client, selection: Selection) -> Option<u64> {
    match client
        .execute(client.prepare(Command::ProbeNetwork {
            selection: selection.clone(),
        }))
        .await
    {
        Ok(Output::NetworkLatency(latency)) if latency.selection == selection => latency.millis,
        _ => None,
    }
}

fn probe(
    jobs: &mut JoinSet<Option<Event>>,
    seen: &mut Vec<Selection>,
    client: Arc<Client>,
    selection: Selection,
    limit: Arc<tokio::sync::Semaphore>,
) -> bool {
    if seen.contains(&selection) {
        return false;
    }
    seen.push(selection.clone());
    jobs.spawn(async move {
        let Ok(_permit) = limit.acquire_owned().await else {
            return Some(Event::Timing(selection, Timing::Measured(None)));
        };
        let millis = measure(&client, selection.clone()).await;
        Some(Event::Timing(selection, Timing::Measured(millis)))
    });
    true
}

async fn observe(client: Arc<Client>, selected: Option<Selection>, sender: mpsc::Sender<Event>) {
    let mut jobs = JoinSet::new();
    let mut seen = Vec::new();
    let limit = Arc::new(tokio::sync::Semaphore::new(4));
    let cached = listing(&client, false).await;
    let mut pending = selected.into_iter().collect::<Vec<_>>();
    if let Some(cached) = cached {
        pending.extend(cached.presets.iter().map(|preset| preset.selection.clone()));
        if sender.send(Event::Catalog(cached)).await.is_err() {
            return;
        }
    }
    for selection in pending {
        if probe(
            &mut jobs,
            &mut seen,
            client.clone(),
            selection.clone(),
            limit.clone(),
        ) && sender
            .send(Event::Timing(selection, Timing::Pending))
            .await
            .is_err()
        {
            return;
        }
    }
    let refresh = client.clone();
    jobs.spawn(async move { listing(&refresh, true).await.map(Event::Catalog) });
    while let Some(event) = jobs.join_next().await {
        let Ok(Some(event)) = event else {
            continue;
        };
        let pending = match &event {
            Event::Catalog(listing) => listing
                .presets
                .iter()
                .map(|preset| preset.selection.clone())
                .collect(),
            _ => Vec::new(),
        };
        if sender.send(event).await.is_err() {
            return;
        }
        for selection in pending {
            if probe(
                &mut jobs,
                &mut seen,
                client.clone(),
                selection.clone(),
                limit.clone(),
            ) && sender
                .send(Event::Timing(selection, Timing::Pending))
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

pub(super) fn custom(
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    selection: Selection,
    debounce: Duration,
    revision: u64,
    window: &mut Window,
    cx: &mut Context<Picker>,
) -> Read {
    let stop = CancellationToken::new();
    let cancelled = stop.clone();
    let expected = selection.clone();
    let job = runtime.spawn(async move {
        tokio::select! {
            biased;
            _ = cancelled.cancelled() => None,
            millis = async {
                tokio::time::sleep(debounce).await;
                measure(&client, selection).await
            } => Some(millis),
        }
    });
    let task = cx.spawn_in(window, async move |owner, cx| {
        let result = job.await;
        let _ = owner.update_in(cx, |picker, window, cx| {
            if picker.custom_revision != revision
                || !picker.custom(cx)
                || picker
                    .selection(cx)
                    .map(super::super::edit::desired)
                    .as_ref()
                    != Some(&expected)
            {
                return;
            }
            let millis = match result {
                Ok(Some(millis)) => millis,
                Ok(None) => return,
                Err(_) => None,
            };
            if let Some(item) = picker
                .items
                .iter_mut()
                .find(|item| item.choice == Choice::Custom)
            {
                item.timing = Timing::Measured(millis);
            }
            picker.sync(window, cx);
        });
    });
    Read { stop, _task: task }
}
