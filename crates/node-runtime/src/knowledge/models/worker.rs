use super::{Fault, Input, Model, Models, Output, Work, cancelled, files, unavailable};
use sailry_link::CancellationToken;
use std::sync::Weak;
use tokio::{
    sync::{mpsc, oneshot},
    time::{Duration, Instant},
};

pub(super) enum Control {
    Unload {
        model: Option<Model>,
        reply: Option<oneshot::Sender<()>>,
    },
}

struct Loaded {
    model: Model,
    epoch: u64,
    stamp: files::Stamp,
    used: Instant,
    engine: sailry_embedding::Engine,
}

pub(super) enum Next {
    Control(Control),
    Work(Work),
    Idle,
    Stop,
}

pub(super) async fn next(
    queries: &mut mpsc::Receiver<Work>,
    indexes: &mut mpsc::Receiver<Work>,
    controls: &mut mpsc::Receiver<Control>,
    stop: &CancellationToken,
    deadline: Instant,
) -> Next {
    tokio::select! {
        biased;
        _ = stop.cancelled() => Next::Stop,
        control = controls.recv() => control.map_or(Next::Stop, Next::Control),
        query = queries.recv() => query.map_or(Next::Stop, Next::Work),
        index = indexes.recv() => index.map_or(Next::Stop, Next::Work),
        _ = tokio::time::sleep_until(deadline) => Next::Idle,
    }
}

pub(super) async fn run(
    owner: Weak<Models>,
    mut queries: mpsc::Receiver<Work>,
    mut indexes: mpsc::Receiver<Work>,
    mut controls: mpsc::Receiver<Control>,
    stop: CancellationToken,
    idle: Duration,
) {
    let mut loaded: Option<Loaded> = None;
    loop {
        let deadline = loaded
            .as_ref()
            .map_or_else(|| Instant::now() + idle, |loaded| loaded.used + idle);
        match next(&mut queries, &mut indexes, &mut controls, &stop, deadline).await {
            Next::Stop => break,
            Next::Idle => unload(&mut loaded).await,
            Next::Control(Control::Unload { model, reply }) => {
                if loaded
                    .as_ref()
                    .is_some_and(|loaded| model.is_none_or(|model| loaded.model == model))
                {
                    unload(&mut loaded).await;
                }
                if let Some(reply) = reply {
                    let _ = reply.send(());
                }
            }
            Next::Work(work) => {
                let Some(owner) = owner.upgrade() else {
                    break;
                };
                let result = execute(&owner, &work, &mut loaded).await;
                let _ = work.reply.send(result);
            }
        }
    }
    unload(&mut loaded).await;
    for receive in [&mut queries, &mut indexes] {
        while let Ok(work) = receive.try_recv() {
            let _ = work.reply.send(Err(cancelled()));
        }
    }
}

async fn unload(loaded: &mut Option<Loaded>) {
    if let Some(loaded) = loaded.take() {
        let _ = tokio::task::spawn_blocking(move || drop(loaded)).await;
    }
}

async fn execute(
    owner: &Models,
    work: &Work,
    loaded: &mut Option<Loaded>,
) -> Result<Output, Fault> {
    if work.reply.is_closed() {
        return Err(cancelled());
    }
    owner.refresh().await?;
    owner.current(work)?;
    let previous = loaded.take();
    let directory = files::directory(&owner.root, work.model);
    let model = work.model;
    let format = work.format;
    let epoch = work.epoch;
    let stamp = work.stamp.clone();
    let input = work.input.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let mut loaded = match previous {
            Some(loaded)
                if loaded.model == model && loaded.epoch == epoch && loaded.stamp == stamp =>
            {
                loaded
            }
            previous => {
                drop(previous);
                let engine =
                    sailry_embedding::Engine::load_format(format, &directory).map_err(failure)?;
                Loaded {
                    model,
                    epoch,
                    stamp,
                    used: Instant::now(),
                    engine,
                }
            }
        };
        let result = match input {
            Input::Encode(texts) => {
                let inputs = texts.iter().map(String::as_str).collect::<Vec<_>>();
                loaded
                    .engine
                    .encode(&inputs)
                    .map(Output::Vectors)
                    .map_err(failure)
            }
            Input::Offsets(text) => loaded
                .engine
                .offsets(&text)
                .map(Output::Offsets)
                .map_err(failure),
        };
        loaded.used = Instant::now();
        Ok::<_, Fault>((loaded, result))
    })
    .await
    .map_err(|_| unavailable())??;
    *loaded = Some(outcome.0);
    owner.refresh().await?;
    if let Err(error) = owner.current(work) {
        if error.code == sailry_protocol::ErrorCode::RevisionConflict {
            unload(loaded).await;
        }
        return Err(error);
    }
    outcome.1
}

pub(super) fn failure(error: sailry_embedding::Error) -> Fault {
    match error {
        sailry_embedding::Error::TooLong => {
            Fault::invalid("embedding input exceeds the token window")
        }
        _ => Fault::new(sailry_protocol::ErrorCode::Unavailable, error.to_string()),
    }
}
