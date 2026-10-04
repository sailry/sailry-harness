use sailry_protocol::plugin::schema;

fn main() {
    let schema = match std::env::args().nth(1).as_deref() {
        Some("manifest") => schema::manifest(),
        None | Some("extension") => schema::extension(),
        _ => {
            eprintln!("usage: plugin_schema [extension|manifest]");
            std::process::exit(2);
        }
    };
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
