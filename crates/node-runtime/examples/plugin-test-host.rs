//! Explicit development bridge; no profile, network, or filesystem SDK services.
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    const MAX_INPUT: u64 = 8 * 1024 * 1024;
    std::io::stdin()
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err("plugin fixture input exceeds limit".into());
    }
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => {
            let input = serde_json::from_slice(&bytes)?;
            let result = sailry_node_runtime::plugin_test::run(input);
            serde_json::to_writer(std::io::stdout().lock(), &result)?;
        }
        [argument] if argument == "--translation" => {
            let input = serde_json::from_slice(&bytes)?;
            let result = sailry_node_runtime::plugin_test::translation::run(input);
            serde_json::to_writer(std::io::stdout().lock(), &result)?;
        }
        [argument] if argument == "--validate-schema" => {
            let input = serde_json::from_slice(&bytes)?;
            let result = sailry_node_runtime::plugin_test::schema::validate(input);
            serde_json::to_writer(std::io::stdout().lock(), &result)?;
        }
        _ => return Err("unsupported plugin test host arguments".into()),
    }
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}
