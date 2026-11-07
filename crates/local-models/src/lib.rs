//! Node-owned local model registry, CPU inference scheduling, OCR and decision services.
pub mod decision;
pub mod installation;
pub mod models;
pub mod ocr;

use sailry_office as office;
use sailry_runtime_core::{files, profile};
