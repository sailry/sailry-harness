package codeindex

import (
	"context"
	"errors"
	"slices"
	"strings"
	"testing"
)

// outlined is one expected symbol: language, kind, name and depth.
type outlined struct {
	language, kind, name string
	depth                uint16
}

const (
	vueFixture  = "<template><div>中文</div></template>\n<script lang=\"ts\">interface 用户 { value: string }\nfunction greet(user: 用户) { return user.value; }\n</script>\n<style lang=\"scss\">$accent: red; .notice { color: $accent; }</style>\n"
	htmlFixture = "<p>中文</p>\n<script type=\"application/typescript\">function hello(name: string) { return name; }</script>\n<style>.notice { color: red; }</style>"
)

// TestCodeOutline replays the outline fixtures of the Rust code index.
func TestCodeOutline(t *testing.T) {
	var parser Parser
	ctx := context.Background()
	for _, fixture := range []struct {
		path, source, language string
		capabilities           Capabilities
		symbols                []outlined
	}{
		{
			"schema.sql", "CREATE TABLE accounts (id INTEGER PRIMARY KEY);\nCREATE VIEW active AS SELECT id FROM accounts;\n", "sql",
			Capabilities{Structure: true},
			[]outlined{{"sql", "struct", "accounts", 0}, {"sql", "struct", "active", 0}},
		},
		{
			"handler.php", "<?php\nfunction handle_request($request) { return $request; }\n", "php",
			Capabilities{Structure: true, Symbols: true},
			[]outlined{{"php", "function", "handle_request", 0}},
		},
		{"index.html", "<!doctype html><html><body><h1>Title</h1></body></html>\n", "html", Capabilities{}, nil},
		{"style.css", ".notice { color: red; }\n", "css", Capabilities{}, nil},
		{"style.scss", "$accent: red; .notice { color: $accent; }\n", "scss", Capabilities{}, nil},
		{
			"main.rs", "pub struct State;\npub fn run() -> State { State }\n", "rust",
			Capabilities{Structure: true, Symbols: true},
			[]outlined{{"rust", "struct", "State", 0}, {"rust", "function", "run", 0}},
		},
		{
			"settings.json", "{\"service\":{\"password\":\"value-stays-in-source\",\"port\":8080}}\n", "json",
			Capabilities{Data: true},
			[]outlined{{"json", "data", "service", 1}, {"json", "data", "password", 2}, {"json", "data", "port", 2}},
		},
		{
			"settings.toml", "[service]\nport = 8080\n", "toml",
			Capabilities{Data: true},
			[]outlined{{"toml", "data", "service", 1}, {"toml", "data", "port", 2}},
		},
		{
			"settings.yaml", "service:\n  port: 8080\n", "yaml",
			Capabilities{Data: true},
			[]outlined{{"yaml", "data", "service", 1}, {"yaml", "data", "port", 2}},
		},
		{
			"App.vue", vueFixture, "vue",
			Capabilities{Structure: true, Symbols: true, Embedded: true},
			[]outlined{{"typescript", "interface", "用户", 0}, {"typescript", "function", "greet", 0}},
		},
		{
			"index.html", htmlFixture, "html",
			Capabilities{Structure: true, Symbols: true, Embedded: true},
			[]outlined{{"typescript", "function", "hello", 0}},
		},
		{"notes.unknown-language", "plain text\n", "", Capabilities{}, nil},
	} {
		t.Run(fixture.path, func(t *testing.T) {
			parsed, err := parser.Parse(ctx, fixture.path, fixture.source)
			if err != nil {
				t.Fatal(err)
			}
			status := StatusParsed
			if fixture.language == "" {
				status = StatusUnsupported
			}
			if parsed.Status != status || parsed.Language != fixture.language || parsed.Hash != Hash(fixture.source) {
				t.Fatalf("status %q language %q hash %q", parsed.Status, parsed.Language, parsed.Hash)
			}
			if parsed.Capabilities != fixture.capabilities || parsed.Partial || parsed.Truncated {
				t.Fatalf("capabilities %+v partial %v truncated %v", parsed.Capabilities, parsed.Partial, parsed.Truncated)
			}
			var symbols []outlined
			for _, symbol := range parsed.Symbols {
				symbols = append(symbols, outlined{symbol.Language, symbol.Kind, symbol.Name, symbol.Depth})
				start, end := int(symbol.Range.StartByte), int(symbol.Range.EndByte)
				if start > end || end > len(fixture.source) || symbol.Kind != "data" && !strings.Contains(fixture.source[start:end], symbol.Name) {
					t.Fatalf("%s range %d..%d", symbol.Name, start, end)
				}
				if symbol.Signature == nil || len(*symbol.Signature) > maxSignatureBytes || symbol.FileRevision != parsed.Hash {
					t.Fatalf("%s signature %v revision %q", symbol.Name, symbol.Signature, symbol.FileRevision)
				}
				// Configuration values stay in the source.
				if metadata := symbol.Name + symbol.Kind + *symbol.Signature; symbol.Kind == "data" &&
					(strings.ContainsAny(metadata, "{}") || strings.Contains(metadata, "8080") || strings.Contains(metadata, "value-stays")) {
					t.Fatalf("data symbol leaks a value: %q", metadata)
				}
			}
			if !slices.Equal(symbols, fixture.symbols) {
				t.Fatalf("symbols %v, want %v", symbols, fixture.symbols)
			}
		})
	}

	// Embedded regions keep the host file's UTF-8 coordinates.
	for _, embedded := range []struct{ path, source, name, text string }{
		{"App.vue", vueFixture, "用户", "interface 用户"},
		{"index.html", htmlFixture, "hello", "function hello"},
	} {
		parsed, err := parser.Parse(ctx, embedded.path, embedded.source)
		if err != nil {
			t.Fatal(err)
		}
		index := slices.IndexFunc(parsed.Symbols, func(symbol Symbol) bool { return symbol.Name == embedded.name })
		position := strings.Index(embedded.source, embedded.text)
		column := position - strings.Index(embedded.source, "\n") - 1
		if got := parsed.Symbols[index].Range; int(got.StartByte) != position || got.StartLine != 1 || int(got.StartColumn) != column {
			t.Fatalf("%s starts at %+v, want byte %d line 1 column %d", embedded.name, got, position, column)
		}
	}

	long, err := parser.Parse(ctx, "main.rs", "pub fn function() { /*"+strings.Repeat("中文", 1000)+"*/ }")
	if err != nil || len(long.Symbols) == 0 {
		t.Fatalf("long declaration: %v %v", long.Symbols, err)
	}
	for _, symbol := range long.Symbols {
		if len(*symbol.Signature) > maxSignatureBytes {
			t.Fatalf("signature holds %d bytes", len(*symbol.Signature))
		}
	}
	if got := short("中文🙂", 7); got != "中文" {
		t.Fatalf("short = %q", got)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := parser.Parse(cancelled, "main.rs", "fn run() {}"); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled parse returned %v", err)
	}
}

// referenced is one expected reference: name, kind, scope name and receiver.
type referenced struct {
	name, kind, scope string
	receiver          bool
}

// TestCodeReferences replays the reference fixtures of the Rust code index.
func TestCodeReferences(t *testing.T) {
	var parser Parser
	for _, fixture := range []struct {
		path, source string
		contains     []referenced
		imports      []string
	}{
		{
			"src/index.rs",
			"use crate::store::Handle;\n\nstruct Index;\n\nimpl Index {\n    fn open() -> Self {\n        Self::prepare();\n        helper(1);\n        Index\n    }\n    fn prepare() {}\n}\n\nfn helper(value: u8) -> u8 {\n    let total = compute(value);\n    total\n}\n",
			[]referenced{{"prepare", "call", "open", true}, {"helper", "call", "open", false}, {"compute", "call", "helper", false}},
			[]string{"store", "store/Handle"},
		},
		{
			"src/page.ts",
			"import { render as draw } from './view';\nexport class Page {\n  show(): void {\n    this.paint();\n    draw(this);\n  }\n  paint(): void {}\n}\nfunction start(): Page {\n  return new Page();\n}\n",
			[]referenced{{"paint", "call", "show", true}, {"render", "call", "show", false}, {"Page", "class", "start", false}, {"render", "import", "", false}},
			[]string{"src/view"},
		},
		{
			"app/service.py",
			"from .models import User as Account\n\nclass Service:\n    def run(self):\n        self.save()\n        return build(Account)\n\n    def save(self):\n        pass\n\ndef build(kind):\n    return kind()\n",
			[]referenced{{"save", "call", "run", true}, {"build", "call", "run", false}, {"kind", "call", "build", false}, {"User", "import", "", false}},
			[]string{"app/models", "app/models/User"},
		},
		{
			"cmd/server.go",
			"package main\n\nimport \"example.com/app/store\"\n\ntype Server struct{}\n\nfunc (s *Server) Start() {\n\ts.listen()\n\tstore.Open()\n}\n\nfunc (s *Server) listen() {}\n",
			[]referenced{{"listen", "call", "Start", false}, {"Open", "call", "Start", false}},
			[]string{"example.com/app/store"},
		},
		{"settings.json", "{\"service\":{\"port\":8080}}", nil, nil},
	} {
		t.Run(fixture.path, func(t *testing.T) {
			parsed, err := parser.Parse(context.Background(), fixture.path, fixture.source)
			if err != nil {
				t.Fatal(err)
			}
			var found []referenced
			for _, usage := range parsed.References {
				scope := ""
				if usage.Scope != nil {
					scope = usage.Scope.Name
				}
				found = append(found, referenced{usage.Name, usage.Kind, scope, usage.Receiver})
				// A definition's own name is not a reference to itself.
				if usage.Name == "open" {
					t.Fatalf("definition reported as a reference: %+v", usage)
				}
				if usage.Name == "compute" && (usage.Line != 14 || usage.Column != 16) {
					t.Fatalf("compute at %d:%d", usage.Line, usage.Column)
				}
			}
			for _, expected := range fixture.contains {
				if !slices.Contains(found, expected) {
					t.Fatalf("missing %+v in %+v", expected, found)
				}
			}
			if fixture.contains == nil && len(found) != 0 {
				t.Fatalf("unexpected references %+v", found)
			}
			if !slices.Equal(parsed.Imports, fixture.imports) {
				t.Fatalf("imports %v, want %v", parsed.Imports, fixture.imports)
			}
		})
	}

	for _, module := range []struct {
		language, path, statement string
		items, keys               []string
	}{
		{"python", "app/views/home.py", "from ..models import user", []string{"user"}, []string{"app/models", "app/models/user"}},
		{"python", "main.py", "import os.path, numpy as np", nil, []string{"numpy", "os/path"}},
		{"typescript", "src/views/home.ts", "import { render } from '../lib/render.js';", []string{"render"}, []string{"src/lib/render"}},
		{"typescript", "src/main.ts", "import api from '@/services/api'", nil, []string{"services/api"}},
		{"rust", "src/main.rs", "use crate::code_index::{index::Index, parser};", nil, []string{"code_index", "code_index/index/Index", "code_index/parser"}},
		{"rust", "src/main.rs", "pub(crate) use super::store::Handle;", nil, []string{"store", "store/Handle"}},
		{"go", "cmd/main.go", "\"example.com/project/internal/store\"", nil, []string{"example.com/project/internal/store"}},
		{"java", "src/App.java", "import static com.example.util.Strings.trim;", nil, []string{"com/example/util/Strings", "com/example/util/Strings/trim"}},
	} {
		if got := modules(module.language, module.path, []statement{{source: module.statement, items: module.items}}); !slices.Equal(got, module.keys) {
			t.Fatalf("%s %q: keys %v, want %v", module.language, module.statement, got, module.keys)
		}
	}
	renamed := aliases([]statement{
		{source: "import { render as draw } from './view'", items: []string{"render"}, alias: "draw"},
		{source: "import * as views from './view'", alias: "views"},
		{source: "from pkg.mod import helper", items: []string{"helper"}},
	})
	if len(renamed) != 1 || renamed["draw"] != "render" {
		t.Fatalf("aliases %v", renamed)
	}
	if last("crate::index::Index") != "Index" || last(`App\Models\User`) != "User" || last("plain") != "plain" {
		t.Fatal("last segment")
	}
	query := "; leading comment (ignored)\n(call function: (identifier) @name) @reference.call\n\n((identifier) @name (#match? @name \"^[(]\"))\n[(a) (b)] @x ; trailing\n"
	if got, want := patterns(query), []string{
		"(call function: (identifier) @name) @reference.call",
		"((identifier) @name (#match? @name \"^[(]\"))",
		"[(a) (b)] @x ; trailing",
	}; !slices.Equal(got, want) {
		t.Fatalf("patterns %q", got)
	}
	for source, expected := range map[string]bool{
		"self.save": true, "this.paint": true, "Self::new": true, "$this->load": true, "myself.save": false, "other.save": false,
	} {
		if got := receiver(source, strings.LastIndexAny(source, ".:>")+1); got != expected {
			t.Fatalf("receiver(%q) = %v", source, got)
		}
	}
	if !identifier("valid_name?") || !identifier("名前") || identifier("") || identifier("a b") || identifier("call()") {
		t.Fatal("identifier")
	}
}
