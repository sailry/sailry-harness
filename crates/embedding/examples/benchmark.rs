#[path = "benchmark/process.rs"]
mod process;
#[path = "benchmark/reference.rs"]
mod reference;
#[path = "benchmark/workload.rs"]
mod workload;

use sailry_embedding::{Engine, ModelId};
use serde_json::{Value, json};
use std::{error::Error, path::PathBuf, time::Instant};

const GIB: u64 = 1024 * 1024 * 1024;

fn run() -> Result<bool, Box<dyn Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let model = arguments
        .next()
        .ok_or("usage: benchmark <model-key> <model-directory> [runs]")?;
    let model: ModelId =
        serde_json::from_value(Value::String(model.to_string_lossy().into_owned()))?;
    let directory = PathBuf::from(
        arguments
            .next()
            .ok_or("a verified model directory is required")?,
    );
    let runs = arguments
        .next()
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(5);
    if !(1..=50).contains(&runs) || arguments.next().is_some() {
        return Err("runs must be between 1 and 50; select exactly one model per process".into());
    }
    let initial = process::sample()?;
    let descriptor = model.descriptor();
    let files = descriptor
        .assets
        .iter()
        .map(|asset| sailry_model_assets::File {
            name: asset.name,
            size: asset.bytes,
            sha256: asset.sha256,
        })
        .collect::<Vec<_>>();
    let started = Instant::now();
    if !sailry_model_assets::ready(&directory, &files) {
        return Err("model files do not match the pinned catalog".into());
    }
    let verification_ms = started.elapsed().as_secs_f64() * 1000.;
    let started = Instant::now();
    let mut engine = Engine::load(model, &directory)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    let loaded = process::sample()?;
    let documents = workload::documents(&engine)?;
    let inputs = documents.iter().map(String::as_str).collect::<Vec<_>>();
    let documents = workload::measure(&mut engine, "knowledge-chunks-256", &inputs, runs)?;
    let queries = workload::measure(&mut engine, "natural-queries", &workload::QUERIES, runs)?;
    let references = reference::compare(model, &mut engine)?;
    let final_process = process::sample()?;
    let memory_limit = (model == ModelId::GraniteMulti).then_some(GIB);
    let memory_passed = memory_limit.is_none_or(|limit| final_process.peak_rss_bytes <= limit);
    let passed = memory_passed && references["passed"].as_bool() == Some(true);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "model": model,
            "model_revision": descriptor.revision,
        "backend": if model == ModelId::PotionMulti { "static-table" } else { "onnx" },
        "runtime_version": (model != ModelId::PotionMulti).then_some(sailry_model_assets::runtime::VERSION),
            "dimension": engine.dimension(),
            "os": std::env::consts::OS,
            "architecture": std::env::consts::ARCH,
            "process_id": std::process::id(),
            "available_parallelism": std::thread::available_parallelism()?.get(),
            "isolated_model_process": true,
            "assets_verified": true,
            "asset_verification_ms": verification_ms,
            "load_ms": load_ms,
            "process": {"initial": initial, "loaded": loaded, "final": final_process},
            "workloads": [documents, queries],
            "references": references,
            "memory_acceptance": {"peak_rss_limit_bytes": memory_limit, "passed": memory_passed},
            "passed": passed,
        }))?
    );
    Ok(passed)
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(error) => {
            println!("{}", json!({"passed": false, "error": error.to_string()}));
            std::process::ExitCode::FAILURE
        }
    }
}
