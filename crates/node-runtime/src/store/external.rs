use sailry_link::Response;
use sailry_protocol::{NodeId, Request};
use tokio::sync::oneshot;

pub(super) enum Work {
    PluginExecution(super::plugins::execution::Execution),
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
