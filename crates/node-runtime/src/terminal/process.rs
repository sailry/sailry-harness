mod local;
pub(crate) mod ssh;

use super::{failure, session::Message};
use sailry_protocol::{
    Fault, SshId,
    terminal::{Settings, Tool, Viewport},
};
use std::{
    path::PathBuf,
    sync::{Arc, mpsc::SyncSender},
};

pub(crate) enum Source {
    Local {
        root: PathBuf,
        settings: Settings,
        tool: Option<Tool>,
        environment: Arc<crate::process::Environment>,
        profile: Option<PathBuf>,
        access: sailry_protocol::process::Access,
    },
    Ssh {
        profile: SshId,
        connection: ssh::Connection,
    },
}

impl Source {
    pub fn access(&self) -> Option<sailry_protocol::process::Access> {
        match self {
            Self::Local { access, .. } => Some(*access),
            Self::Ssh { .. } => None,
        }
    }
    pub fn tool(&self) -> Option<Tool> {
        match self {
            Self::Local { tool, .. } => *tool,
            Self::Ssh { .. } => None,
        }
    }

    pub fn profile(&self) -> Option<SshId> {
        match self {
            Self::Local { .. } => None,
            Self::Ssh { profile, .. } => Some(*profile),
        }
    }
}

pub(super) enum Process {
    Local(Box<local::Process>),
    Ssh(ssh::Process),
}

impl Process {
    pub fn start(
        source: Source,
        viewport: &Viewport,
        messages: SyncSender<Message>,
    ) -> Result<Self, Fault> {
        match source {
            source @ Source::Local { .. } => local::Process::start(source, viewport, messages)
                .map(Box::new)
                .map(Self::Local),
            Source::Ssh { connection, .. } => {
                Ok(Self::Ssh(ssh::Process::start(connection, messages)))
            }
        }
    }

    pub fn resize(&self, viewport: &Viewport) -> Result<(), Fault> {
        match self {
            Self::Local(process) => process.resize(viewport),
            Self::Ssh(process) => process.resize(viewport),
        }
    }

    pub fn write(&self, bytes: Vec<u8>) -> Result<(), Fault> {
        match self {
            Self::Local(process) => process.write(bytes),
            Self::Ssh(process) => process.write(bytes),
        }
    }

    pub fn try_wait(&mut self) -> Result<Option<u32>, Fault> {
        match self {
            Self::Local(process) => process
                .child
                .try_wait()
                .map(|status| status.map(|status| status.exit_code()))
                .map_err(failure),
            Self::Ssh(process) => process.try_wait(),
        }
    }
}
