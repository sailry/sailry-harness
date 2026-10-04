//! Registration separates tool execution from its controller-facing presentation.
use adk_core::Tool;
use sailry_protocol::tool::Presentation;
use sailry_protocol::{ErrorCode, Fault};
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct Registration {
    pub tool: Arc<dyn Tool>,
    pub plugin: Option<String>,
    pub presentation: Presentation,
    pub grouping: sailry_protocol::tool::Grouping,
    pub display: Option<(String, sailry_protocol::tool::Display)>,
    /// Core workflow tools enforce their own policy rather than file/process approval.
    pub managed: bool,
}

impl Registration {
    pub fn new(tool: Arc<dyn Tool>, presentation: Presentation) -> Self {
        Self {
            tool,
            plugin: None,
            presentation,
            grouping: Default::default(),
            display: None,
            managed: false,
        }
    }

    pub fn managed(tool: Arc<dyn Tool>) -> Self {
        Self {
            tool,
            plugin: None,
            presentation: Presentation::default(),
            grouping: Default::default(),
            display: None,
            managed: true,
        }
    }
}

impl From<Arc<dyn Tool>> for Registration {
    fn from(tool: Arc<dyn Tool>) -> Self {
        Self::new(tool, Presentation::default())
    }
}

#[derive(Default)]
pub(super) struct Catalog {
    presentations: BTreeMap<String, Presentation>,
    groupings: BTreeMap<String, sailry_protocol::tool::Grouping>,
    displays: BTreeMap<String, sailry_protocol::tool::Display>,
}

impl Catalog {
    pub fn register(&mut self, registration: Registration) -> Result<Arc<dyn Tool>, Fault> {
        if self.presentations.contains_key(registration.tool.name()) {
            return Err(Fault::new(
                ErrorCode::InvalidRequest,
                "tool registration has a duplicate name",
            ));
        }
        self.groupings
            .insert(registration.tool.name().into(), registration.grouping);
        if let Some((name, display)) = registration.display {
            self.displays.insert(name, display);
        }
        self.presentations
            .insert(registration.tool.name().into(), registration.presentation);
        Ok(registration.tool)
    }

    pub fn groupings(&self) -> BTreeMap<String, sailry_protocol::tool::Grouping> {
        self.groupings.clone()
    }

    pub fn displays(&self) -> BTreeMap<String, sailry_protocol::tool::Display> {
        self.displays.clone()
    }

    pub fn presentations(self) -> BTreeMap<String, Presentation> {
        self.presentations
    }
}
