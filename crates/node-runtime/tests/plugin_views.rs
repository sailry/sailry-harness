use super::plugins::support::{execute, fixture, info, install};
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{plugin::Info, *};
use std::{fs, path::Path};
#[path = "plugins/views.rs"]
mod views;
