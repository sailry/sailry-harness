use sailry_link::Response;
use sailry_protocol::{NodeId, Request};
use tokio::sync::oneshot;

pub(super) enum Work {
    Network(super::network::Execution),
    PluginExecution(Box<super::plugins::execution::Execution>),
    Login(super::login::Operation),
    Process(super::processes::Execution),
    Resources(super::resources::Execution),
    Ssh(super::ssh::worker::Execution),
    Database(super::databases::worker::Execution),
    Mutation(super::mutations::Mutation),
    Session(super::forwarding::Transfer),
}

pub(super) struct Completed {
    pub caller: NodeId,
    pub request: Request,
    pub result: Response,
    pub reply: oneshot::Sender<Response>,
}

/// Return ownership when an execution worker has not received this operation.
pub(super) fn reject(work: Work) -> (NodeId, Request, oneshot::Sender<Response>) {
    match work {
        Work::Network(work) => (work.caller, work.request, work.reply),
        Work::PluginExecution(work) => (work.caller, work.request, work.reply),
        Work::Login(work) => (work.caller, work.request, work.reply),
        Work::Process(work) => (work.caller, work.request, work.reply),
        Work::Resources(work) => (work.caller, work.request, work.reply),
        Work::Ssh(work) => (work.caller, work.request, work.reply),
        Work::Database(work) => (work.caller, work.request, work.reply),
        Work::Mutation(work) => (work.caller, work.request, work.reply),
        Work::Session(work) => (work.caller, work.request, work.reply),
    }
}
