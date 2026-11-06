use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

/// An installed distribution on the execution Node, not a terminal shell or a paired Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Distribution {
    pub name: String,
    pub version: u32,
}
