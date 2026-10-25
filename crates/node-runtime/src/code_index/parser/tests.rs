use super::*;
use std::sync::Arc;

fn parser() -> Arc<Parser> {
    super::super::resources::Resources::bundled()
        .unwrap()
        .parser
}

#[test]
fn loads_real_bundled_grammars() {
    let parser = parser();
    let stop = CancellationToken::new();
    for (path, source, language) in [
        (
            "schema.sql",
            "CREATE TABLE accounts (id INTEGER PRIMARY KEY);\nCREATE VIEW active AS SELECT id FROM accounts;\n",
            "sql",
        ),
        (
            "handler.php",
            "<?php\nfunction handle_request($request) { return $request; }\n",
            "php",
        ),
        (
            "index.html",
            "<!doctype html><html><body><h1>Title</h1></body></html>\n",
            "html",
        ),
        ("style.css", ".notice { color: red; }\n", "css"),
        (
            "style.scss",
            "$accent: red; .notice { color: $accent; }\n",
            "scss",
        ),
        (
            "main.rs",
            "pub struct State;\npub fn run() -> State { State }\n",
            "rust",
        ),
        ("settings.json", "{\"service\":{\"port\":8080}}\n", "json"),
        ("settings.toml", "[service]\nport = 8080\n", "toml"),
        ("settings.yaml", "service:\n  port: 8080\n", "yaml"),
    ] {
        let parsed = parser.parse(path.into(), source, &stop).unwrap();
        assert_eq!(parsed.status, Status::Parsed, "{path}");
        assert_eq!(parsed.language.as_deref(), Some(language), "{path}");
        let hash = blake3::hash(source.as_bytes()).to_hex().to_string();
        assert_eq!(parsed.hash.as_deref(), Some(hash.as_str()));
        let actual = parser.process(source, language).unwrap();
        assert_eq!(
            parsed.capabilities.structure,
            !actual.structure.is_empty(),
            "{path}"
        );
        assert_eq!(
            parsed.capabilities.symbols,
            !actual.symbols.is_empty(),
            "{path}"
        );
        assert_eq!(parsed.capabilities.data, actual.data.is_some(), "{path}");
        for symbol in &parsed.symbols {
            assert!(
                source
                    .get(symbol.range.start_byte as usize..symbol.range.end_byte as usize)
                    .is_some(),
                "{path}: {}",
                symbol.name
            );
            assert!(
                symbol
                    .signature
                    .as_ref()
                    .is_none_or(|text| text.len() <= MAX_SIGNATURE_BYTES)
            );
        }
    }
}

#[test]
fn extracts_sql_declarations_and_configuration_keys() {
    let parser = parser();
    let sql = parser
        .parse(
            "schema.sql".into(),
            "CREATE TABLE accounts (id INTEGER);",
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(
        sql.symbols
            .iter()
            .any(|symbol| symbol.name.contains("accounts"))
    );
    let config = parser
        .parse(
            "settings.json".into(),
            "{\"service\":{\"port\":8080}}",
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(config.capabilities.data);
    assert!(config.symbols.iter().any(|symbol| symbol.name == "port"));
}

#[test]
fn keeps_configuration_values_out_of_symbols() {
    let source = r#"{"service":{"password":"value-stays-in-source","port":8080}}"#;
    let parsed = parser()
        .parse("settings.json".into(), source, &CancellationToken::new())
        .unwrap();
    assert!(parsed.capabilities.data);
    assert!(parsed.symbols.iter().any(|symbol| symbol.name == "port"));
    let metadata = serde_json::to_string(&parsed.symbols).unwrap();
    assert!(!metadata.contains("value-stays-in-source"));
    assert!(!metadata.contains("8080"));
    assert!(parsed.symbols.iter().all(|symbol| {
        symbol
            .signature
            .as_ref()
            .is_some_and(|text| text.len() <= MAX_SIGNATURE_BYTES && !text.contains(['{', '}']))
    }));
}

#[test]
fn preserves_embedded_utf8_coordinates() {
    let parser = parser();
    let stop = CancellationToken::new();
    let vue = "<template><div>中文</div></template>\n<script lang=\"ts\">interface 用户 { value: string }\nfunction greet(user: 用户) { return user.value; }\n</script>\n<style lang=\"scss\">$accent: red; .notice { color: $accent; }</style>\n";
    let parsed = parser.parse("view.vue".into(), vue, &stop).unwrap();
    assert!(parsed.capabilities.embedded);
    let interface = parsed
        .symbols
        .iter()
        .find(|symbol| symbol.language == "typescript" && symbol.name == "用户")
        .expect("embedded TypeScript interface");
    let position = vue.find("interface 用户").unwrap();
    assert_eq!(interface.range.start_byte as usize, position);
    assert_eq!(interface.range.start_line, 1);
    assert_eq!(
        interface.range.start_column as usize,
        position - vue.find('\n').unwrap() - 1
    );
    assert_eq!(
        interface.file_revision,
        blake3::hash(vue.as_bytes()).to_hex().to_string()
    );
    assert!(
        vue[interface.range.start_byte as usize..interface.range.end_byte as usize]
            .contains("用户")
    );
    let html = "<p>中文</p>\n<script type=\"application/typescript\">function hello(name: string) { return name; }</script>\n<style>.notice { color: red; }</style>";
    let parsed = parser.parse("index.html".into(), html, &stop).unwrap();
    let function = parsed
        .symbols
        .iter()
        .find(|symbol| symbol.language == "typescript" && symbol.name == "hello")
        .expect("embedded HTML script");
    assert_eq!(
        function.range.start_byte as usize,
        html.find("function hello").unwrap()
    );
    assert_eq!(function.range.start_line, 1);
}

#[test]
fn limits_signatures_without_splitting_utf8() {
    let source = format!("pub fn function() {{ /*{}*/ }}", "中文".repeat(1000));
    let parsed = parser()
        .parse("main.rs".into(), &source, &CancellationToken::new())
        .unwrap();
    assert!(parsed.symbols.iter().all(|symbol| {
        symbol
            .signature
            .as_ref()
            .is_none_or(|signature| signature.len() <= MAX_SIGNATURE_BYTES)
    }));
    assert_eq!(short("中文🙂", 7), "中文");
}

#[test]
fn cancels_before_loading_or_parsing() {
    let stop = CancellationToken::new();
    stop.cancel();
    assert_eq!(
        parser()
            .parse("main.rs".into(), "fn run() {}", &stop)
            .err()
            .unwrap()
            .code,
        ErrorCode::Cancelled
    );
}
