//! Execution-device permissions for native computer operations.
use crate::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ScreenCapture,
    Accessibility,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    pub node: NodeId,
    pub platform: String,
    /// Only the execution device's local client can open its permission settings.
    pub local: bool,
    /// None means that permission detection is not supported on this platform.
    pub screen_capture: Option<bool>,
    pub accessibility: Option<bool>,
}
