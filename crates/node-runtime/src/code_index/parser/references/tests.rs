use super::*;

fn parse(path: &str, source: &str) -> Parsed {
    super::super::super::resources::Resources::bundled()
        .unwrap()
        .parser
        .parse(path.into(), source, &CancellationToken::new())
        .unwrap()
}

/// Name, kind, enclosing declaration and receiver of each reference.
fn summary(parsed: &Parsed) -> Vec<(&str, &str, Option<&str>, bool)> {
    parsed
        .references
        .iter()
        .map(|usage| {
            (
                usage.name.as_str(),
                usage.kind.as_str(),
                usage.scope.as_ref().map(|scope| scope.name.as_str()),
                usage.receiver,
            )
        })
        .collect()
}

#[test]
fn finds_rust_calls_with_scopes() {
    let parsed = parse(
        "src/index.rs",
        "use crate::store::Handle;\n\nstruct Index;\n\nimpl Index {\n    fn open() -> Self {\n        Self::prepare();\n        helper(1);\n        Index\n    }\n    fn prepare() {}\n}\n\nfn helper(value: u8) -> u8 {\n    let total = compute(value);\n    total\n}\n",
    );
    let found = summary(&parsed);
    assert!(
        found.contains(&("prepare", "call", Some("open"), true)),
        "{found:?}"
    );
    assert!(
        found.contains(&("helper", "call", Some("open"), false)),
        "{found:?}"
    );
    assert!(
        found.contains(&("compute", "call", Some("helper"), false)),
        "{found:?}"
    );
    assert!(found.iter().all(|(name, ..)| *name != "open"), "{found:?}");
    assert_eq!(parsed.imports, ["store", "store/Handle"]);
    let compute = parsed
        .references
        .iter()
        .find(|usage| usage.name == "compute")
        .unwrap();
    assert_eq!((compute.line, compute.column), (14, 16));
}

#[test]
fn composes_typescript_with_javascript_calls() {
    let parsed = parse(
        "src/page.ts",
        "import { render as draw } from './view';\nexport class Page {\n  show(): void {\n    this.paint();\n    draw(this);\n  }\n  paint(): void {}\n}\nfunction start(): Page {\n  return new Page();\n}\n",
    );
    let found = summary(&parsed);
    assert!(
        found.contains(&("paint", "call", Some("show"), true)),
        "{found:?}"
    );
    assert!(
        found.contains(&("render", "call", Some("show"), false)),
        "{found:?}"
    );
    assert!(
        found.contains(&("Page", "class", Some("start"), false)),
        "{found:?}"
    );
    assert!(
        found.contains(&("render", "import", None, false)),
        "{found:?}"
    );
    assert_eq!(parsed.imports, ["src/view"]);
}

#[test]
fn resolves_python_aliases_and_receivers() {
    let parsed = parse(
        "app/service.py",
        "from .models import User as Account\n\nclass Service:\n    def run(self):\n        self.save()\n        return build(Account)\n\n    def save(self):\n        pass\n\ndef build(kind):\n    return kind()\n",
    );
    let found = summary(&parsed);
    assert!(
        found.contains(&("save", "call", Some("run"), true)),
        "{found:?}"
    );
    assert!(
        found.contains(&("build", "call", Some("run"), false)),
        "{found:?}"
    );
    assert!(
        found.contains(&("kind", "call", Some("build"), false)),
        "{found:?}"
    );
    assert!(
        found.contains(&("User", "import", None, false)),
        "{found:?}"
    );
    assert_eq!(parsed.imports, ["app/models", "app/models/User"]);
}

#[test]
fn finds_go_method_calls() {
    let parsed = parse(
        "cmd/server.go",
        "package main\n\nimport \"example.com/app/store\"\n\ntype Server struct{}\n\nfunc (s *Server) Start() {\n\ts.listen()\n\tstore.Open()\n}\n\nfunc (s *Server) listen() {}\n",
    );
    let found = summary(&parsed);
    assert!(
        found.contains(&("listen", "call", Some("Start"), false)),
        "{found:?}"
    );
    assert!(
        found.contains(&("Open", "call", Some("Start"), false)),
        "{found:?}"
    );
    assert_eq!(parsed.imports, ["example.com/app/store"]);
}

#[test]
fn omits_languages_without_tags() {
    let parsed = parse("settings.json", "{\"service\":{\"port\":8080}}");
    assert!(parsed.references.is_empty());
    assert!(parsed.imports.is_empty());
    assert!(!supported("json"));
    assert!(supported("rust") && supported("typescript") && supported("python"));
}

#[test]
fn splits_query_patterns() {
    let source = "; leading comment (ignored)\n(call function: (identifier) @name) @reference.call\n\n((identifier) @name (#match? @name \"^[(]\"))\n[(a) (b)] @x ; trailing\n";
    assert_eq!(
        patterns(source),
        [
            "(call function: (identifier) @name) @reference.call",
            "((identifier) @name (#match? @name \"^[(]\"))",
            "[(a) (b)] @x ; trailing",
        ]
    );
}

#[test]
fn recognizes_receivers_and_identifiers() {
    for (source, expected) in [
        ("self.save", true),
        ("this.paint", true),
        ("Self::new", true),
        ("$this->load", true),
        ("myself.save", false),
        ("other.save", false),
    ] {
        let start = source.rfind(['.', ':', '>']).unwrap() + 1;
        assert_eq!(receiver(source, start), expected, "{source}");
    }
    assert!(identifier("valid_name?") && identifier("名前"));
    assert!(!identifier("") && !identifier("a b") && !identifier("call()"));
}
