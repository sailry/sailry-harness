#![cfg(unix)]
use super::*;
use crate::approvals::rules::{completed, execute, observe, restart, route, submit};
use git2::{Oid, Repository, Signature};
use process::Fixture;
use sailry_protocol::plugin;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[path = "review/concurrency.rs"]
mod concurrency;
#[path = "review/evidence.rs"]
mod evidence;
#[path = "review/fixture.rs"]
mod fixture;
#[path = "review/frozen.rs"]
mod frozen;
#[path = "review/scope.rs"]
mod scope;

use fixture::*;
