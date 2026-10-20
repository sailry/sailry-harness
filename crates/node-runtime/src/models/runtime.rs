//! One bounded CPU inference scheduler shared by Node-owned model kinds.
use super::registry::{Resource, cancelled, failure, files};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, models::Kind};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Duration, Instant},
};

const IDLE: Duration = Duration::from_secs(300);
const MEMORY_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lane {
    Interactive,
    Background,
}

pub(crate) enum Engine {
    Embedding(Box<sailry_embedding::Engine>),
    Ocr(Box<sailry_ocr::Engine>),
    Decision(Box<sailry_decision::Engine>),
}

impl Engine {
    pub(super) fn load(resource: &Resource) -> Result<Self, Fault> {
        match resource.definition.format.kind() {
            Kind::Embedding => {
                sailry_embedding::Engine::load_definition(&resource.definition, &resource.directory)
                    .map(|engine| Self::Embedding(Box::new(engine)))
                    .map_err(embedding_error)
            }
            Kind::Ocr => sailry_ocr::Engine::load(&resource.definition, &resource.directory)
                .map(|engine| Self::Ocr(Box::new(engine)))
                .map_err(failure),
            Kind::Decision => {
                sailry_decision::Engine::load(&resource.definition, &resource.directory)
                    .map(|engine| Self::Decision(Box::new(engine)))
                    .map_err(crate::decision::error)
            }
            Kind::Speech => Err(Fault::invalid("speech belongs to the controller")),
        }
    }
}

pub(crate) fn embedding_error(error: sailry_embedding::Error) -> Fault {
    match error {
        sailry_embedding::Error::TooLong => {
            Fault::invalid("embedding input exceeds the token window")
        }
        error => failure(error),
    }
}

type Check = Arc<dyn Fn() -> Result<(), Fault> + Send + Sync>;
type Call = Box<dyn FnOnce(Result<&mut Engine, Fault>) -> bool + Send>;

struct Work {
    resource: Resource,
    cancel: CancellationToken,
    check: Check,
    call: Call,
    validating: bool,
}

struct Unload {
    id: String,
    reply: oneshot::Sender<()>,
}

struct Loaded {
    resource: Resource,
    used: Instant,
    bytes: u64,
    engine: Engine,
}

pub(crate) struct Runtime {
    interactive: mpsc::Sender<Work>,
    background: mpsc::Sender<Work>,
    control: mpsc::Sender<Unload>,
    stop: CancellationToken,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct Pending(CancellationToken);
impl Drop for Pending {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl Runtime {
    pub(crate) fn new(stop: CancellationToken) -> Self {
        let (interactive, interactive_jobs) = mpsc::channel(8);
        let (background, background_jobs) = mpsc::channel(4);
        let (control, controls) = mpsc::channel(4);
        let stopping = stop.clone();
        let handle = tokio::runtime::Handle::current();
        // OCR's synchronous document pipeline and async embedding clients share this
        // one worker. It reuses the Node executor's timer and cancellation driver.
        let worker = std::thread::Builder::new()
            .name("sailry-local-models".into())
            .spawn(move || {
                sailry_model_assets::session::background();
                handle.block_on(run(interactive_jobs, background_jobs, controls, stopping));
            })
            .expect("local model scheduler starts");
        Self {
            interactive,
            background,
            control,
            stop,
            worker: Mutex::new(Some(worker)),
        }
    }

    fn submit<T: Send + 'static>(
        &self,
        resource: Resource,
        lane: Lane,
        cancel: &CancellationToken,
        check: Check,
        validating: bool,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<(Pending, oneshot::Receiver<Result<T, Fault>>), Fault> {
        if cancel.is_cancelled() || self.stop.is_cancelled() {
            return Err(cancelled());
        }
        let token = self.stop.child_token();
        let pending = Pending(token.clone());
        let (reply, result) = oneshot::channel();
        let checking = check.clone();
        let stopping = token.clone();
        let work = Work {
            resource,
            cancel: token,
            check,
            validating,
            call: Box::new(move |engine| {
                let result = engine.and_then(|engine| operation(engine, &stopping));
                let result = if stopping.is_cancelled() {
                    Err(cancelled())
                } else {
                    checking().and(result)
                };
                let success = result.is_ok();
                let _ = reply.send(result);
                success
            }),
        };
        let queue = match lane {
            Lane::Interactive => &self.interactive,
            Lane::Background => &self.background,
        };
        queue.try_send(work).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => {
                Fault::new(ErrorCode::Busy, "local inference queue is full")
            }
            mpsc::error::TrySendError::Closed(_) => failure("local inference stopped"),
        })?;
        Ok((pending, result))
    }

    pub(crate) async fn execute<T: Send + 'static>(
        &self,
        resource: Resource,
        lane: Lane,
        cancel: &CancellationToken,
        check: Check,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<T, Fault> {
        self.request(resource, lane, cancel, check, false, operation)
            .await
    }

    pub(super) async fn validate<T: Send + 'static>(
        &self,
        resource: Resource,
        cancel: &CancellationToken,
        check: Check,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<T, Fault> {
        self.request(resource, Lane::Background, cancel, check, true, operation)
            .await
    }

    async fn request<T: Send + 'static>(
        &self,
        resource: Resource,
        lane: Lane,
        cancel: &CancellationToken,
        check: Check,
        validating: bool,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<T, Fault> {
        let (_pending, result) =
            self.submit(resource, lane, cancel, check, validating, operation)?;
        tokio::select! {biased;
            _=cancel.cancelled()=>Err(cancelled()),
            _=self.stop.cancelled()=>Err(cancelled()),
            result=result=>result.map_err(|_|failure("local inference stopped"))?,
        }
    }

    pub(crate) fn blocking<T: Send + 'static>(
        &self,
        resource: Resource,
        lane: Lane,
        cancel: &CancellationToken,
        check: Check,
        operation: impl FnOnce(&mut Engine, &CancellationToken) -> Result<T, Fault> + Send + 'static,
    ) -> Result<T, Fault> {
        let (_pending, result) = self.submit(resource, lane, cancel, check, false, operation)?;
        futures::executor::block_on(async {
            tokio::select! {biased;
                _=cancel.cancelled()=>Err(cancelled()),
                _=self.stop.cancelled()=>Err(cancelled()),
                result=result=>result.map_err(|_|failure("local inference stopped"))?,
            }
        })
    }

    pub(crate) async fn unload(&self, id: &str) -> Result<(), Fault> {
        let (reply, result) = oneshot::channel();
        self.control
            .send(Unload {
                id: id.into(),
                reply,
            })
            .await
            .map_err(|_| cancelled())?;
        result.await.map_err(|_| cancelled())
    }

    pub(crate) async fn shutdown(&self) {
        self.stop.cancel();
        let worker = self
            .worker
            .lock()
            .expect("local model worker lock is healthy")
            .take();
        if let Some(worker) = worker {
            let _ = tokio::task::spawn_blocking(move || worker.join()).await;
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

async fn run(
    mut interactive: mpsc::Receiver<Work>,
    mut background: mpsc::Receiver<Work>,
    mut control: mpsc::Receiver<Unload>,
    stop: CancellationToken,
) {
    let mut loaded = BTreeMap::<String, Loaded>::new();
    loop {
        let deadline = loaded
            .values()
            .map(|entry| entry.used + IDLE)
            .min()
            .unwrap_or_else(|| Instant::now() + IDLE);
        match next(
            &mut interactive,
            &mut background,
            &mut control,
            &stop,
            deadline,
        )
        .await
        {
            Next::Stop => break,
            Next::Unload(unload) => {
                loaded.remove(&unload.id);
                let _ = unload.reply.send(());
            }
            Next::Work(work) => execute(work, &mut loaded, &stop),
            Next::Idle => expire(&mut loaded, Instant::now()),
        }
    }
    drop(loaded);
    for queue in [&mut interactive, &mut background] {
        while let Ok(work) = queue.try_recv() {
            (work.call)(Err(cancelled()));
        }
    }
}

enum Next {
    Stop,
    Unload(Unload),
    Work(Work),
    Idle,
}

async fn next(
    interactive: &mut mpsc::Receiver<Work>,
    background: &mut mpsc::Receiver<Work>,
    control: &mut mpsc::Receiver<Unload>,
    stop: &CancellationToken,
    deadline: Instant,
) -> Next {
    tokio::select! {biased;
        _=stop.cancelled()=>Next::Stop,
        unload=control.recv()=>unload.map_or(Next::Stop,Next::Unload),
        work=interactive.recv()=>work.map_or(Next::Stop,Next::Work),
        work=background.recv()=>work.map_or(Next::Stop,Next::Work),
        _=tokio::time::sleep_until(deadline)=>Next::Idle,
    }
}

fn execute(work: Work, loaded: &mut BTreeMap<String, Loaded>, stop: &CancellationToken) {
    let id = work.resource.id.clone();
    let validating = work.validating;
    let result = acquire(&work, loaded, stop);
    let success = match result {
        Ok(entry) => {
            let success = (work.call)(Ok(&mut entry.engine));
            entry.used = Instant::now();
            success
        }
        Err(error) => (work.call)(Err(error)),
    };
    if validating && !success {
        loaded.remove(&id);
    }
}

fn acquire<'a>(
    work: &Work,
    loaded: &'a mut BTreeMap<String, Loaded>,
    stop: &CancellationToken,
) -> Result<&'a mut Loaded, Fault> {
    if work.cancel.is_cancelled() || stop.is_cancelled() {
        return Err(cancelled());
    }
    (work.check)()?;
    let stamp = if work.validating {
        sailry_model_assets::verified::candidate(
            &work.resource.directory,
            &files::assets(&work.resource.definition),
        )
    } else {
        files::stamp(
            &work.resource.directory,
            &work.resource.id,
            &work.resource.definition,
        )
    };
    if stamp.as_ref() != Some(&work.resource.stamp) {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "local model assets changed",
        ));
    }
    if loaded.get(&work.resource.id).is_some_and(|entry| {
        entry.resource.stamp != work.resource.stamp || entry.resource.epoch != work.resource.epoch
    }) {
        loaded.remove(&work.resource.id);
    }
    if !loaded.contains_key(&work.resource.id) {
        let bytes = work
            .resource
            .definition
            .assets
            .iter()
            .map(|asset| asset.bytes)
            .sum::<u64>();
        make_room(loaded, bytes);
        let engine = Engine::load(&work.resource)?;
        loaded.insert(
            work.resource.id.clone(),
            Loaded {
                resource: work.resource.clone(),
                used: Instant::now(),
                bytes,
                engine,
            },
        );
    }
    loaded
        .get_mut(&work.resource.id)
        .ok_or_else(|| failure("local model did not load"))
}

fn make_room(loaded: &mut BTreeMap<String, Loaded>, bytes: u64) {
    while !loaded.is_empty()
        && loaded
            .values()
            .map(|entry| entry.bytes)
            .sum::<u64>()
            .saturating_add(bytes)
            > MEMORY_BYTES
    {
        let id = loaded
            .iter()
            .min_by_key(|(_, entry)| entry.used)
            .map(|(id, _)| id.clone())
            .expect("loaded engine exists");
        loaded.remove(&id);
    }
}

fn expire(loaded: &mut BTreeMap<String, Loaded>, now: Instant) {
    loaded.retain(|_, entry| entry.used + IDLE > now);
}
