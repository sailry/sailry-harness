package protocol

import (
	"bytes"
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

func TestFixtureRoundTrip(t *testing.T) {
	t.Parallel()
	eachFixture(t, func(t *testing.T, _ string, original []byte, target func() any) {
		value := target()
		if err := json.Unmarshal(original, value); err != nil {
			t.Fatalf("decode Rust fixture: %v", err)
		}
		encoded, err := json.Marshal(value)
		if err != nil {
			t.Fatalf("encode protocol value: %v", err)
		}
		if !reflect.DeepEqual(fixtureJSON(t, original), fixtureJSON(t, encoded)) {
			t.Fatalf("Rust JSON changed\nwant: %s\n got: %s", original, encoded)
		}
	})
}

// eachFixture runs check as a parallel subtest for every JSON fixture, with
// the fixture's name, its bytes and a constructor of its concrete protocol
// type.
func eachFixture(t *testing.T, check func(t *testing.T, name string, original []byte, target func() any)) {
	t.Helper()
	factories := map[string]func() any{
		"browser_action":        func() any { return new(BrowserAction) },
		"command":               func() any { return new(Command) },
		"decision_answer":       func() any { return new(DecisionAnswer) },
		"decision_request":      func() any { return new(DecisionRequest) },
		"effort":                func() any { return new(Effort) },
		"event":                 func() any { return new(Event) },
		"fault":                 func() any { return new(Fault) },
		"installation_resource": func() any { return new(InstallationResource) },
		"mcp_server":            func() any { return new(PluginMcpServer) },
		"output":                func() any { return new(Output) },
		"progress":              func() any { return new(ConversationProgress) },
		"question_spec":         func() any { return new(ConversationQuestionSpec) },
		"role_model":            func() any { return new(RoleModel) },
		"team_settings":         func() any { return new(TeamSettings) },
		"topic":                 func() any { return new(Topic) },
	}
	fixtures := 0
	err := filepath.WalkDir("testdata", func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() || filepath.Ext(path) != ".json" {
			return nil
		}
		relative, err := filepath.Rel("testdata", path)
		if err != nil {
			return err
		}
		name := filepath.ToSlash(relative)
		category, _, _ := strings.Cut(name, "/")
		factory, exists := factories[category]
		if !exists {
			t.Errorf("fixture %s has no concrete protocol type", name)
			return nil
		}
		fixtures++
		name = strings.TrimSuffix(name, ".json")
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			original, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			check(t, name, original, factory)
		})
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if fixtures == 0 {
		t.Fatal("no protocol JSON fixtures found")
	}
}

func fixtureJSON(t *testing.T, data []byte) any {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		t.Fatalf("normalize fixture JSON: %v", err)
	}
	return value
}

func TestCommandKinds(t *testing.T) {
	t.Parallel()
	data, err := os.ReadFile("testdata/commands.txt")
	if err != nil {
		t.Fatal(err)
	}
	want := strings.Fields(string(data))
	if len(want) != 362 {
		t.Fatalf("Rust command inventory contains %d entries, want 362", len(want))
	}
	got := CommandKinds()
	if extensions := []string{"apply_git_hunk", "synthesize_speech", "count_feature"}; len(got) != len(want)+len(extensions) || !reflect.DeepEqual(got[:len(want)], want) || !reflect.DeepEqual(got[len(want):], extensions) {
		t.Fatalf("command kinds differ from the Rust inventory plus the documented native extensions\nwant: %v + %v\n got: %v", want, extensions, got)
	}
	t.Run("native Git hunk extension validates captured revisions", func(t *testing.T) {
		input := CommandApplyGitHunkPayload{Worktree: NewWorktreeID(), Path: "source.go", Scope: GitDiffScopeUnstaged, Operation: GitHunkStage, Hunk: 0, DiffRevision: strings.Repeat("a", 64), ExpectedRevision: strings.Repeat("b", 64), ExpectedIndex: strings.Repeat("c", 64)}
		command := Command{Kind: "apply_git_hunk", Data: input}
		body, err := json.Marshal(command)
		if err != nil {
			t.Fatal(err)
		}
		var decoded Command
		if err = json.Unmarshal(body, &decoded); err != nil {
			t.Fatal(err)
		}
		if !decoded.Durable() || decoded.Data.(*CommandApplyGitHunkPayload).Hunk != 0 {
			t.Fatal("native hunk lost durable identity")
		}
		for _, operation := range []GitHunkOperation{GitHunkStage, GitHunkReject} {
			value := GitHunkApplied{Worktree: input.Worktree, Path: input.Path, Operation: operation, Revision: input.ExpectedRevision}
			if operation == GitHunkStage {
				value.Index = &GitIndex{Revision: input.ExpectedIndex}
			}
			encoded, err := json.Marshal(Output{Kind: "git_hunk_applied", Data: value})
			if err != nil {
				t.Fatal(err)
			}
			var output Output
			if err = json.Unmarshal(encoded, &output); err != nil {
				t.Fatalf("native hunk result roundtrip: %v", err)
			}
		}
		for name, invalid := range map[string]string{
			"unknown field":         strings.Replace(string(body), `"path":"source.go"`, `"path":"source.go","extra":true`, 1),
			"missing nullable head": strings.Replace(string(body), `"expected_head":null,`, "", 1),
			"invalid operation":     strings.Replace(string(body), `"operation":"stage"`, `"operation":"discard_all"`, 1),
			"staged scope":          strings.Replace(string(body), `"scope":"unstaged"`, `"scope":"staged"`, 1),
			"negative hunk":         strings.Replace(string(body), `"hunk":0`, `"hunk":-1`, 1),
		} {
			t.Run(name, func(t *testing.T) {
				if json.Unmarshal([]byte(invalid), new(Command)) == nil {
					t.Fatal("invalid hunk command was accepted")
				}
			})
		}
	})
}

func TestDenyUnknownFields(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name    string
		valid   string
		invalid string
		target  func() any
	}{
		{
			name:    "retained record",
			valid:   `{"kind":"retired","data":null}`,
			invalid: `{"kind":"retired","data":null,"extra":true}`,
			target:  func() any { return new(RetainedRecord) },
		},
		{
			name:    "nested retained record",
			valid:   `{"kind":"retained_record","data":{"kind":"retired","data":{"future":true}}}`,
			invalid: `{"kind":"retained_record","data":{"kind":"retired","data":{"future":true},"extra":true}}`,
			target:  func() any { return new(Output) },
		},
		{
			name:    "team settings",
			valid:   `{"revision":0,"child_concurrency":8}`,
			invalid: `{"revision":0,"child_concurrency":8,"other":true}`,
			target:  func() any { return new(TeamSettings) },
		},
		{
			name:    "progress",
			valid:   `{"title":null,"steps":[]}`,
			invalid: `{"title":null,"steps":[],"status":"active"}`,
			target:  func() any { return new(ConversationProgress) },
		},
		{
			name:    "progress step",
			valid:   `{"title":null,"steps":[{"description":"Inspect","state":"pending"}]}`,
			invalid: `{"title":null,"steps":[{"description":"Inspect","state":"pending","extra":true}]}`,
			target:  func() any { return new(ConversationProgress) },
		},
		{
			name:    "internal network selection",
			valid:   `{"kind":"probe_network","data":{"selection":{"kind":"custom","relays":[]}}}`,
			invalid: `{"kind":"probe_network","data":{"selection":{"kind":"custom","relays":[],"extra":true}}}`,
			target:  func() any { return new(Command) },
		},
		{
			name:    "status scope",
			valid:   `{"kind":"status","data":{"worktree":null,"session":null}}`,
			invalid: `{"kind":"status","data":{"worktree":null,"session":null,"extra":true}}`,
			target:  func() any { return new(Topic) },
		},
		{
			name: "duplicate struct field", valid: `{"code":"busy","message":"fixture"}`,
			invalid: `{"code":"busy","code":"unavailable","message":"fixture"}`,
			target:  func() any { return new(Fault) },
		},
		{
			name: "duplicate discriminator", valid: `{"kind":"browser_completed"}`,
			invalid: `{"kind":"browser_completed","kind":"browser_completed"}`,
			target:  func() any { return new(Output) },
		},
		{
			name: "internal duplicate field", valid: `{"kind":"number","integer":false}`,
			invalid: `{"kind":"number","integer":false,"integer":true}`,
			target:  func() any { return new(ConversationQuestionFormInput) },
		},
		{
			name: "numeric token", valid: `{"kind":"number","integer":false,"minimum":1,"maximum":null}`,
			invalid: `{"kind":"number","integer":false,"minimum":"1","maximum":null}`,
			target:  func() any { return new(ConversationQuestionFormInput) },
		},
		{
			name: "untagged null", valid: `"reicon:home-line"`, invalid: `null`,
			target: func() any { return new(PluginDesktopIcon) },
		},
		{
			name: "vector null item", valid: `{"columns":["A"],"rows":[["value"]]}`,
			invalid: `{"columns":["A"],"rows":[[null]]}`,
			target:  func() any { return new(ToolContentTable) },
		},
		{
			name: "flattened chart", valid: `{"type":"line","data":[{"label":"A","value":1}]}`,
			invalid: `{"title":null,"type":"line","data":[{"label":"A","value":1}]}`,
			target:  func() any { return new(Chart) },
		},
		{
			name: "fixed array length", valid: `{"effort":"default","overall":1,"axes":[1,2,3,4,5]}`,
			invalid: `{"effort":"default","overall":1,"axes":[1,2,3,4]}`,
			target:  func() any { return new(ConversationCatalogScoresEffortScore) },
		},
	}
	for _, item := range cases {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			if err := json.Unmarshal([]byte(item.valid), item.target()); err != nil {
				t.Fatalf("valid control rejected: %v", err)
			}
			if err := json.Unmarshal([]byte(item.invalid), item.target()); err == nil {
				t.Fatal("invalid shape accepted")
			}
		})
	}
}

func TestModelAuthWarning(t *testing.T) {
	t.Parallel()
	polling := `"flow":{"kind":"polling","start":{"url":"https://example.test/start"},"state":"/state","verification_url":"/url","token":{"url":"https://example.test/token"},"success":{"pointer":"/code","values":[0]}}`
	t.Run("a declaration without one decodes as before and omits it", func(t *testing.T) {
		var declaration PluginModelAuthDeclaration
		if err := json.Unmarshal([]byte(`{"id":"account","display":{"label":"Account"},`+polling+`}`), &declaration); err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(declaration)
		if err != nil || declaration.Warning != nil || bytes.Contains(encoded, []byte(`"warning"`)) {
			t.Fatalf("absent warning: %v %s", err, encoded)
		}
	})
	t.Run("a declared one round trips and refuses unknown members", func(t *testing.T) {
		source := `{"id":"account","display":{"label":"Account"},` + polling + `,"warning":{"text":"Risk","locales":{"zh-CN":"风险"}}}`
		var declaration, again PluginModelAuthDeclaration
		if err := json.Unmarshal([]byte(source), &declaration); err != nil {
			t.Fatal(err)
		}
		if declaration.Warning == nil || declaration.Warning.Text != "Risk" || declaration.Warning.Locales["zh-CN"] != "风险" {
			t.Fatalf("warning: %+v", declaration.Warning)
		}
		encoded, err := json.Marshal(declaration)
		if err != nil || json.Unmarshal(encoded, &again) != nil || !reflect.DeepEqual(declaration, again) {
			t.Fatalf("round trip: %v %s", err, encoded)
		}
		var warning PluginModelAuthWarning
		if json.Unmarshal([]byte(`{"text":"Risk","title":"extra"}`), &warning) == nil {
			t.Fatal("authored warning accepted an unknown member")
		}
		if json.Unmarshal([]byte(`{"locales":{}}`), &warning) == nil {
			t.Fatal("warning without text was accepted")
		}
	})
	t.Run("a presets entry carries it and an older peer's entry has none", func(t *testing.T) {
		older := `{"source":{"kind":"builtin"},"preset":` + string(must(json.Marshal(DefaultConversationPresetsPreset()))) + `,"provenance":"bundled","package":null,"origin":null}`
		var entry ConversationPresetsEntry
		if err := DecodeAs(PeerData, []byte(older), &entry); err != nil || entry.LoginWarning != nil {
			t.Fatalf("older entry: %v %+v", err, entry.LoginWarning)
		}
		entry.LoginWarning = &PluginModelAuthWarning{Text: "Risk", Locales: map[string]string{"zh-CN": "风险"}}
		encoded, err := json.Marshal(entry)
		if err != nil {
			t.Fatal(err)
		}
		var again ConversationPresetsEntry
		if err := DecodeAs(PeerData, encoded, &again); err != nil || !reflect.DeepEqual(entry.LoginWarning, again.LoginWarning) {
			t.Fatalf("entry round trip: %v %s", err, encoded)
		}
		// A newer peer's member this build does not know is ignored.
		widened := bytes.Replace(encoded, []byte(`"text":"Risk"`), []byte(`"text":"Risk","severity":"high"`), 1)
		if err := DecodeAs(PeerData, widened, &again); err != nil || again.LoginWarning.Text != "Risk" {
			t.Fatalf("widened entry: %v", err)
		}
	})
	t.Run("a preset declares one, strictly when authored and tolerantly from a peer", func(t *testing.T) {
		plain := `{"id":"local","name":"Local","routes":[{"protocol":"plugin","endpoint":"https://example.test"}],"auth":{"kind":"none"}`
		var preset, again ConversationPresetsPreset
		if err := json.Unmarshal([]byte(plain+`}`), &preset); err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(preset)
		if err != nil || preset.Warning != nil || bytes.Contains(encoded, []byte(`"warning"`)) {
			t.Fatalf("absent warning: %v %s", err, encoded)
		}
		if err := json.Unmarshal([]byte(plain+`,"warning":{"text":"Risk","locales":{"zh-CN":"风险"}}}`), &preset); err != nil {
			t.Fatal(err)
		}
		if preset.Warning == nil || preset.Warning.Text != "Risk" || preset.Warning.Locales["zh-CN"] != "风险" {
			t.Fatalf("warning: %+v", preset.Warning)
		}
		if encoded, err = json.Marshal(preset); err != nil || json.Unmarshal(encoded, &again) != nil || !reflect.DeepEqual(preset, again) {
			t.Fatalf("round trip: %v %s", err, encoded)
		}
		if json.Unmarshal([]byte(plain+`,"warning":{"text":"Risk","title":"extra"}}`), &again) == nil {
			t.Fatal("authored preset warning accepted an unknown member")
		}
		// A newer peer's preset may widen the warning; it still decodes.
		widened := bytes.Replace(encoded, []byte(`"text":"Risk"`), []byte(`"text":"Risk","severity":"high"`), 1)
		if err := DecodeAs(PeerData, widened, &again); err != nil || again.Warning == nil || again.Warning.Text != "Risk" {
			t.Fatalf("widened preset: %v", err)
		}
	})
}

func must(data []byte, err error) []byte {
	if err != nil {
		panic(err)
	}
	return data
}
