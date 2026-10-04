use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{plugin::Info, *};
use std::{fs, path::Path};
use support::{execute, fixture, info, install};

#[path = "plugins/support.rs"]
mod support;
#[path = "plugins/views.rs"]
mod views;
