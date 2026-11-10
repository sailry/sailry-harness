//go:build integration

package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"charm.land/fantasy"
	gogit "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/codeindex"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// acceptanceWait bounds every wait of the B12 acceptance tests that does not
// load a local model.
const acceptanceWait = 15 * time.Second

// acceptanceCall sends one command through Node.Dispatch and decodes its
// answer, or returns its fault.
func acceptanceCall[T any](t *testing.T, node *Node, command protocol.Command) (T, *protocol.Fault) {
	t.Helper()
	return acceptanceRequest[T](t, node, protocol.NewRequest(node.ID(), command), acceptanceWait)
}

func acceptanceRequest[T any](t *testing.T, node *Node, request protocol.Request, wait time.Duration) (T, *protocol.Fault) {
	t.Helper()
	var result T
	accepted, err := node.Dispatch(t.Context(), node.ID(), request)
	if err != nil {
		var failure *protocol.Fault
		if errors.As(err, &failure) {
			return result, failure
		}
		t.Fatalf("%s was not admitted: %v", request.Command.Kind, err)
	}
	select {
	case response, delivered := <-accepted.Completion:
		if !delivered {
			t.Fatalf("%s lost its completion", request.Command.Kind)
		}
		if response.Fault != nil {
			return result, response.Fault
		}
		// A durable replay returns decoded JSON, so every answer is read as JSON.
		body, err := json.Marshal(response.Output.Data)
		if err == nil && string(body) != "null" {
			err = json.Unmarshal(body, &result)
		}
		if err != nil {
			t.Fatalf("%s output %T: %v", request.Command.Kind, response.Output.Data, err)
		}
	case <-time.After(wait):
		t.Fatalf("%s did not complete within %s", request.Command.Kind, wait)
	}
	return result, nil
}

// acceptanceDo is acceptanceCall for a command that must succeed.
func acceptanceDo[T any](t *testing.T, node *Node, command protocol.Command) T {
	t.Helper()
	result, failure := acceptanceCall[T](t, node, command)
	if failure != nil {
		t.Fatalf("%s: %s %s", command.Kind, failure.Code, failure.Message)
	}
	return result
}

// acceptanceEventually polls a condition for at most acceptanceWait.
func acceptanceEventually(t *testing.T, label string, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(acceptanceWait)
	for !done() {
		if time.Now().After(deadline) {
			t.Fatalf("%s: not within %s", label, acceptanceWait)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func acceptanceFault(t *testing.T, label string, failure *protocol.Fault, code protocol.ErrorCode, message string) {
	t.Helper()
	if failure == nil || failure.Code != code || failure.Message != message {
		t.Fatalf("%s: fault %+v, want %s %q", label, failure, code, message)
	}
}

// acceptanceStep is one scripted model response: a tool call, or the final
// answer when tool is empty.
type acceptanceStep struct {
	id, tool  string
	arguments func() string
}

// acceptanceScript is a language model that plays fixed steps and records the
// tools each request offered.
type acceptanceScript struct {
	fantasy.LanguageModel
	mu      sync.Mutex
	steps   []acceptanceStep
	next    int
	offered [][]string
	prompt  fantasy.Prompt
}

// answered returns the tool result text the model was given for a call. A
// step's arguments read it while the script holds its lock.
func (script *acceptanceScript) answered(call string) string {
	for _, message := range script.prompt {
		for _, part := range message.Content {
			if result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part); found && result.ToolCallID == call {
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					return text.Text
				}
			}
		}
	}
	return ""
}

func (script *acceptanceScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	script.mu.Lock()
	defer script.mu.Unlock()
	script.prompt = call.Prompt
	if script.next >= len(script.steps) {
		return nil, errors.New("unexpected extra acceptance model request")
	}
	step := script.steps[script.next]
	script.next++
	names := make([]string, 0, len(call.Tools))
	for _, tool := range call.Tools {
		names = append(names, tool.GetName())
	}
	script.offered = append(script.offered, names)
	part := fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Done"}
	if step.tool != "" {
		part = fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: step.id, ToolCallName: step.tool, ToolCallInput: step.arguments()}
	}
	return func(yield func(fantasy.StreamPart) bool) {
		if yield(part) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
		}
	}, nil
}

func (script *acceptanceScript) requests() [][]string {
	script.mu.Lock()
	defer script.mu.Unlock()
	return slices.Clone(script.offered)
}

func acceptanceArguments(value any) func() string {
	return func() string {
		body, _ := json.Marshal(value)
		return string(body)
	}
}

// acceptanceNode starts a real Node whose turns run the scripted model.
func acceptanceNode(t *testing.T, profile string, script *acceptanceScript, options Options) *Node {
	t.Helper()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	options.Environment = &environment
	if options.MCP == nil {
		options.MCP = fixtureMCPOptions()
	}
	if script != nil {
		options.Model = func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil }
	}
	node, err := StartWithOptions(t.Context(), profile, options)
	if err != nil {
		t.Fatal(err)
	}
	return node
}

// acceptanceSession registers a fixture provider and opens a conversation in
// the project.
func acceptanceSession(t *testing.T, node *Node, project protocol.ProjectID, permission protocol.Permission) protocol.Session {
	t.Helper()
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Revision, provider.Enabled, provider.Name = protocol.NewProviderID(), 1, true, "Fixture"
	provider.Api, provider.Authentication, provider.Endpoint, provider.DefaultModel = protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey, "http://fixture.invalid/v1", "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID, selected.Context, selected.Output, selected.Tools = "fixture", 200000, 8000, true
	provider.Models = []protocol.ConversationModel{selected}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider.ID, "fixture", protocol.WorkModeCode, permission
	return acceptanceDo[protocol.Session](t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project, Config: &config}})
}

// acceptanceTurn submits one message and runs the turn to completion within
// wait. Every pending approval is handed to approve before it is approved. It
// returns the packages frozen for the turn and the tool responses by call.
func acceptanceTurn(t *testing.T, node *Node, session protocol.Session, text string, wait time.Duration, approve func(protocol.ConversationApproval)) ([]protocol.PluginReference, map[string]*canonical.ToolResponse) {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), wait)
	defer cancel()
	subscription, err := node.Subscribe(ctx, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	queued := acceptanceDo[protocol.QueuedTurn](t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Message: protocol.ConversationInput{Text: text},
	}})
	seen := map[protocol.ApprovalID]bool{}
	for completed := false; !completed; {
		var history protocol.ConversationHistory
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(ctx, db, session.ID, nil, 100)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State != protocol.ConversationApprovalStatePending || seen[approval.ID] {
				continue
			}
			seen[approval.ID] = true
			if approve == nil {
				t.Fatalf("the turn asked for an unexpected approval: %+v", approval)
			}
			approve(approval)
			acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{
				Session: session.ID, Approval: approval.ID, Decision: protocol.ConversationDecision{Kind: "approve"},
			}})
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				completed = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("turn failed: %+v", run)
			}
		}
		if !completed {
			if _, err := subscription.Next(ctx); err != nil {
				t.Fatalf("turn did not complete within %s: %v", wait, err)
			}
		}
	}
	var state store.AgentState
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		var err error
		state, err = store.LoadAgentState(t.Context(), db, session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	results := map[string]*canonical.ToolResponse{}
	for _, record := range state.Records {
		if message := record.Message(); message != nil {
			for _, block := range message.Content {
				if response, ok := block.Data.(*canonical.ToolResponse); ok {
					results[response.ID] = response
				}
			}
		}
	}
	return queued.Plugins, results
}

// acceptanceResult decodes a successful tool response.
func acceptanceResult(t *testing.T, results map[string]*canonical.ToolResponse, call, tool string) map[string]any {
	t.Helper()
	response := results[call]
	if response == nil {
		t.Fatalf("%s returned no response", call)
	}
	if response.Name != tool || response.Status != canonical.ToolSuccess {
		t.Fatalf("%s (%s) status %v: %s", call, response.Name, response.Status, response.Result.Value)
	}
	var value map[string]any
	if err := json.Unmarshal(response.Result.Value, &value); err != nil {
		t.Fatalf("%s result %s: %v", call, response.Result.Value, err)
	}
	return value
}

var codeFixture = map[string]string{
	"cmd/server.go": `package main

// Config holds the listen address.
type Config struct {
	Address string
}

func main() {
	Serve(Config{Address: ":8080"})
}

// Serve starts the listener.
func Serve(config Config) {
	helper(config.Address)
}

func helper(address string) {
	_ = address
}
`,
	"app/service.py": `class Service:
    def run(self):
        return compute(2)


def compute(value):
    return value * 2
`,
	"src/page.ts": `export function format(value: string): string {
  return value.trim();
}

export class Page {
  render(): string {
    return format(" page ");
  }
}
`,
	"lib/index.rs": `pub struct Index {
    pub entries: usize,
}

pub fn build() -> Index {
    open()
}

fn open() -> Index {
    Index { entries: 0 }
}
`,
	"settings.json": `{"name": "fixture", "port": 8080}
`,
	"README.md": "# Fixture\n\nA small project in several languages.\n",
}

// codeProject writes the fixture into a new Git repository with one commit
// and returns its root and commit.
func codeProject(t *testing.T, files map[string]string) (string, string) {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	repository, err := gogit.PlainInitWithOptions(root, &gogit.PlainInitOptions{InitOptions: gogit.InitOptions{DefaultBranch: plumbing.NewBranchReferenceName("main")}})
	if err != nil {
		t.Fatal(err)
	}
	tree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	for name, text := range files {
		path := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(text), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := tree.Add(name); err != nil {
			t.Fatal(err)
		}
	}
	commit, err := tree.Commit("Add fixture", &gogit.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(100, 0)}})
	if err != nil {
		t.Fatal(err)
	}
	return root, commit.String()
}

// codeMain returns the main worktree of a project.
func codeMain(t *testing.T, node *Node, project protocol.ProjectID) protocol.WorktreeID {
	t.Helper()
	for _, tree := range snapshotFixture(t, node).Worktrees {
		if tree.Project != nil && *tree.Project == project && tree.Main {
			return tree.ID
		}
	}
	t.Fatal("project has no main worktree")
	return protocol.WorktreeID{}
}

type codeSymbol struct {
	path, name, kind string
	line             uint32
}

func codeSymbols(symbols []protocol.CodeIndexSymbol) []codeSymbol {
	found := make([]codeSymbol, 0, len(symbols))
	for _, symbol := range symbols {
		found = append(found, codeSymbol{symbol.Path, symbol.Name, symbol.Kind, symbol.Range.StartLine})
	}
	return found
}

func codeSearch(t *testing.T, node *Node, worktree protocol.WorktreeID, query protocol.CodeIndexSearch) protocol.CodeIndexSearchPage {
	t.Helper()
	if query.Limit == 0 {
		query.Limit = 20
	}
	page := acceptanceDo[protocol.CodeIndexSearchPage](t, node, protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: worktree, Query: query}})
	if page.Partial || page.Truncated {
		t.Fatalf("search %+v is incomplete: %+v", query, page)
	}
	return page
}

func codeGraph(t *testing.T, node *Node, worktree protocol.WorktreeID, view protocol.CodeIndexGraphView, name string) protocol.CodeIndexGraphGraphPage {
	t.Helper()
	query := protocol.CodeIndexGraph{View: view, Paths: []string{}, Depth: 3, Limit: 50}
	if name != "" {
		query.Name = &name
	}
	page := acceptanceDo[protocol.CodeIndexGraphGraphPage](t, node, protocol.Command{Kind: "graph_code", Data: protocol.CommandGraphCodePayload{Worktree: worktree, Query: query}})
	if page.View != view || page.Partial || page.Truncated {
		t.Fatalf("graph %s %s is incomplete: %+v", view, name, page)
	}
	return page
}

// codeCallers lists "path:scope" of a graph page's callers.
func codeCallers(page protocol.CodeIndexGraphGraphPage) []string {
	var callers []string
	for _, reference := range page.Callers {
		scope := ""
		if reference.Scope != nil {
			scope = reference.Scope.Name
		}
		callers = append(callers, reference.Path+":"+scope)
	}
	slices.Sort(callers)
	return callers
}

// codeFingerprints reads the distinct rule fingerprints of a closed index.
func codeFingerprints(t *testing.T, database string, replace string) []string {
	t.Helper()
	handle, err := store.OpenHandle(t.Context(), database)
	if err != nil {
		t.Fatal(err)
	}
	defer handle.Close()
	var found []string
	if err := handle.Write(t.Context(), func(db *sql.Conn) error {
		if replace != "" {
			if _, err := db.ExecContext(t.Context(), "UPDATE files SET fingerprint=?1", replace); err != nil {
				return err
			}
		}
		rows, err := db.QueryContext(t.Context(), "SELECT DISTINCT fingerprint FROM files ORDER BY fingerprint")
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var fingerprint string
			if err := rows.Scan(&fingerprint); err != nil {
				return err
			}
			found = append(found, fingerprint)
		}
		return rows.Err()
	}); err != nil {
		t.Fatal(err)
	}
	return found
}

// TestCodeIndexCommands drives the code index commands of a real Node over a
// small project in several languages.
func TestCodeIndexCommands(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	node := acceptanceNode(t, profile, nil, Options{})
	stopped := false
	defer func() {
		if !stopped {
			node.Stop(context.Background())
		}
	}()
	root, commit := codeProject(t, codeFixture)
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Code fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	main := codeMain(t, node, project.ID)
	cache := func(worktree protocol.WorktreeID) string {
		return filepath.Join(profile, "cache", "code-index", worktree.String())
	}

	t.Run("outline", func(t *testing.T) {
		for _, expected := range []struct {
			path, language string
			symbols        []codeSymbol
		}{
			{"cmd/server.go", "go", []codeSymbol{
				{"cmd/server.go", "Config", "type", 3}, {"cmd/server.go", "main", "function", 7},
				{"cmd/server.go", "Serve", "function", 12}, {"cmd/server.go", "helper", "function", 16},
			}},
			{"app/service.py", "python", []codeSymbol{
				{"app/service.py", "Service", "class", 0}, {"app/service.py", "run", "function", 1}, {"app/service.py", "compute", "function", 5},
			}},
			{"src/page.ts", "typescript", []codeSymbol{
				{"src/page.ts", "format", "function", 0}, {"src/page.ts", "Page", "class", 4}, {"src/page.ts", "render", "method", 5},
			}},
			{"lib/index.rs", "rust", []codeSymbol{
				{"lib/index.rs", "Index", "struct", 0}, {"lib/index.rs", "build", "function", 4}, {"lib/index.rs", "open", "function", 8},
			}},
		} {
			outline := acceptanceDo[protocol.CodeIndexOutline](t, node, protocol.Command{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: main, Path: expected.path}})
			source := codeFixture[expected.path]
			if outline.Path != expected.path || outline.Language == nil || *outline.Language != expected.language || outline.Status != codeindex.StatusParsed ||
				outline.FileRevision == nil || *outline.FileRevision != codeindex.Hash(source) || outline.Partial || outline.Truncated || !outline.Capabilities.Symbols {
				t.Fatalf("outline of %s: %+v", expected.path, outline)
			}
			if found := codeSymbols(outline.Symbols); !slices.Equal(found, expected.symbols) {
				t.Fatalf("outline of %s lists %+v, want %+v", expected.path, found, expected.symbols)
			}
			for _, symbol := range outline.Symbols {
				lines := strings.Split(source, "\n")
				if symbol.FileRevision != *outline.FileRevision || !strings.Contains(lines[symbol.Range.StartLine], symbol.Name) ||
					source[symbol.Range.StartByte:symbol.Range.EndByte] == "" || symbol.Range.EndLine < symbol.Range.StartLine {
					t.Fatalf("symbol %+v does not cover its declaration", symbol)
				}
			}
		}
		data := acceptanceDo[protocol.CodeIndexOutline](t, node, protocol.Command{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: main, Path: "settings.json"}})
		if data.Language == nil || *data.Language != "json" || !data.Capabilities.Data {
			t.Fatalf("outline of settings.json: %+v", data)
		}
		_, failure := acceptanceCall[protocol.CodeIndexOutline](t, node, protocol.Command{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: main, Path: "../outside.go"}})
		if failure == nil {
			t.Fatal("an outline outside the worktree was answered")
		}
	})

	var revision uint64
	t.Run("search", func(t *testing.T) {
		all := codeSearch(t, node, main, protocol.CodeIndexSearch{Limit: 100})
		revision = all.IndexRevision
		languages := map[string]bool{}
		for _, symbol := range all.Symbols {
			languages[symbol.Language] = true
		}
		for _, language := range []string{"go", "python", "typescript", "rust"} {
			if !languages[language] {
				t.Fatalf("the index has no %s symbol: %+v", language, codeSymbols(all.Symbols))
			}
		}
		if all.Visited < uint64(len(codeFixture)) || all.Skipped != 0 || len(all.Issues) != 0 || revision == 0 {
			t.Fatalf("index report %+v", all)
		}
		serve := codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "Serve"})
		if found := codeSymbols(serve.Symbols); len(found) == 0 || found[0] != (codeSymbol{"cmd/server.go", "Serve", "function", 12}) {
			t.Fatalf("search for Serve found %+v", found)
		}
		if serve.Symbols[0].Signature == nil || !strings.Contains(*serve.Symbols[0].Signature, "Serve(config Config)") {
			t.Fatalf("Serve has signature %v", serve.Symbols[0].Signature)
		}
		name, language, path := "comp", "python", "lib"
		if found := codeSymbols(codeSearch(t, node, main, protocol.CodeIndexSearch{Name: &name}).Symbols); !slices.Equal(found, []codeSymbol{{"app/service.py", "compute", "function", 5}}) {
			t.Fatalf("name search found %+v", found)
		}
		for _, symbol := range codeSearch(t, node, main, protocol.CodeIndexSearch{Language: &language}).Symbols {
			if symbol.Path != "app/service.py" {
				t.Fatalf("language filter returned %+v", symbol)
			}
		}
		scoped := codeSymbols(codeSearch(t, node, main, protocol.CodeIndexSearch{Path: &path}).Symbols)
		if len(scoped) != 3 || slices.ContainsFunc(scoped, func(symbol codeSymbol) bool { return symbol.path != "lib/index.rs" }) {
			t.Fatalf("path filter returned %+v", scoped)
		}
		if found := codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "nosuchsymbolanywhere"}).Symbols; len(found) != 0 {
			t.Fatalf("an absent name found %+v", codeSymbols(found))
		}
		_, failure := acceptanceCall[protocol.CodeIndexSearchPage](t, node, protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: main, Query: protocol.CodeIndexSearch{Query: strings.Repeat("x", 4097), Limit: 20}}})
		acceptanceFault(t, "oversized search", failure, protocol.ErrorCodeInvalidRequest, "invalid code search limits")
	})

	t.Run("graph", func(t *testing.T) {
		callers := codeGraph(t, node, main, protocol.CodeIndexGraphViewCallers, "helper")
		if len(callers.Targets) != 1 || callers.Targets[0].Path != "cmd/server.go" || callers.Targets[0].StartLine != 16 || !slices.Equal(codeCallers(callers), []string{"cmd/server.go:Serve"}) {
			t.Fatalf("callers of helper: %+v", callers)
		}
		if reference := callers.Callers[0]; reference.Line != 13 || reference.Confidence == nil || reference.Ambiguous {
			t.Fatalf("caller of helper: %+v", reference)
		}
		callees := codeGraph(t, node, main, protocol.CodeIndexGraphViewCallees, "build")
		if len(callees.Callees) != 1 || callees.Callees[0].Name != "open" || len(callees.Callees[0].Targets) != 1 || callees.Callees[0].Targets[0].Path != "lib/index.rs" || callees.Callees[0].Targets[0].StartLine != 8 {
			t.Fatalf("callees of build: %+v", callees.Callees)
		}
		impact := codeGraph(t, node, main, protocol.CodeIndexGraphViewImpact, "format")
		if len(impact.Impact) != 1 || impact.Impact[0].Path != "src/page.ts" || !slices.ContainsFunc(impact.Impact[0].Symbols, func(symbol protocol.CodeIndexGraphAffected) bool { return symbol.Name == "render" }) {
			t.Fatalf("impact of format: %+v", impact.Impact)
		}
		ranked := codeGraph(t, node, main, protocol.CodeIndexGraphViewMap, "")
		paths := []string{}
		for _, entry := range ranked.Map {
			paths = append(paths, entry.Path)
		}
		slices.Sort(paths)
		// The map ranks every indexed file with declarations, data files too.
		if !slices.Equal(paths, []string{"app/service.py", "cmd/server.go", "lib/index.rs", "settings.json", "src/page.ts"}) || ranked.Total != 5 {
			t.Fatalf("map lists %v of %d", paths, ranked.Total)
		}
		_, failure := acceptanceCall[protocol.CodeIndexGraphGraphPage](t, node, protocol.Command{Kind: "graph_code", Data: protocol.CommandGraphCodePayload{Worktree: main, Query: protocol.CodeIndexGraph{View: protocol.CodeIndexGraphViewCallers, Paths: []string{}, Depth: 3, Limit: 50}}})
		acceptanceFault(t, "callers without a symbol", failure, protocol.ErrorCodeInvalidRequest, "invalid code graph query")
	})

	t.Run("incremental refresh", func(t *testing.T) {
		// An edit outside the Node: one new caller, one renamed function.
		edited := strings.Replace(codeFixture["cmd/server.go"], "func main() {", "func Extra() {\n\thelper(\"extra\")\n}\n\nfunc main() {", 1)
		if err := os.WriteFile(filepath.Join(root, "cmd", "server.go"), []byte(edited), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "app", "service.py"), []byte(strings.ReplaceAll(codeFixture["app/service.py"], "compute", "calculate")), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Remove(filepath.Join(root, "lib", "index.rs")); err != nil {
			t.Fatal(err)
		}
		extra := codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "Extra"})
		if found := codeSymbols(extra.Symbols); !slices.Equal(found, []codeSymbol{{"cmd/server.go", "Extra", "function", 7}}) || extra.IndexRevision <= revision {
			t.Fatalf("the edit was not indexed: %+v at revision %d after %d", found, extra.IndexRevision, revision)
		}
		if extra.Symbols[0].FileRevision != codeindex.Hash(edited) {
			t.Fatal("the edited file kept its old revision")
		}
		if found := codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "compute"}).Symbols; len(found) != 0 {
			t.Fatalf("a renamed function is still indexed: %+v", codeSymbols(found))
		}
		if found := codeSymbols(codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "calculate"}).Symbols); !slices.Equal(found, []codeSymbol{{"app/service.py", "calculate", "function", 5}}) {
			t.Fatalf("the renamed function was not indexed: %+v", found)
		}
		if found := codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "build"}).Symbols; len(found) != 0 {
			t.Fatalf("a deleted file is still indexed: %+v", codeSymbols(found))
		}
		if callers := codeCallers(codeGraph(t, node, main, protocol.CodeIndexGraphViewCallers, "helper")); !slices.Equal(callers, []string{"cmd/server.go:Extra", "cmd/server.go:Serve"}) {
			t.Fatalf("callers after the edit: %v", callers)
		}
		outline := acceptanceDo[protocol.CodeIndexOutline](t, node, protocol.Command{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: main, Path: "cmd/server.go"}})
		if len(outline.Symbols) != 5 || outline.FileRevision == nil || *outline.FileRevision != codeindex.Hash(edited) {
			t.Fatalf("outline after the edit: %+v", outline)
		}
		// An edit through the Node's own file command is picked up as well.
		acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "write_file", Data: protocol.CommandWriteFilePayload{Worktree: main, Path: "src/extra.ts", Text: "export function appended(): number {\n  return 1;\n}\n"}})
		if found := codeSymbols(codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "appended"}).Symbols); !slices.Equal(found, []codeSymbol{{"src/extra.ts", "appended", "function", 0}}) {
			t.Fatalf("a file written through the Node was not indexed: %+v", found)
		}
	})

	t.Run("cancel", func(t *testing.T) {
		// A project large enough that its first reconcile is still running
		// when the cancellation arrives.
		files := map[string]string{}
		for file := range 240 {
			var source strings.Builder
			fmt.Fprintf(&source, "package bulk\n\n")
			for function := range 300 {
				fmt.Fprintf(&source, "func Bulk%dFunction%d(value int) int {\n\tif value > %d {\n\t\treturn value * %d\n\t}\n\treturn value + %d\n}\n\n", file, function, function, file, function)
			}
			files[fmt.Sprintf("bulk/file%03d.go", file)] = source.String()
		}
		bulkRoot, _ := codeProject(t, files)
		bulk := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
			Name: "Bulk fixture", Path: bulkRoot, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
		}})
		worktree := codeMain(t, node, bulk.ID)
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: worktree, Query: protocol.CodeIndexSearch{Query: "Bulk", Limit: 20}}})
		accepted, err := node.Dispatch(t.Context(), node.ID(), request)
		if err != nil {
			t.Fatal(err)
		}
		key := codeQueryKey{node.ID(), request.ID}
		deadline := time.Now().Add(acceptanceWait)
		for running := false; !running; {
			node.codeIndex.mu.Lock()
			_, running = node.codeIndex.running[key]
			node.codeIndex.mu.Unlock()
			if !running {
				select {
				case response := <-accepted.Completion:
					t.Fatalf("the query finished before it could be cancelled: %+v", response.Fault)
				default:
				}
				if time.Now().After(deadline) {
					t.Fatal("the query never started")
				}
				time.Sleep(time.Millisecond)
			}
		}
		started := time.Now()
		cancelled := acceptanceDo[protocol.OutputCodeIndexQueryCancelledPayload](t, node, protocol.Command{Kind: "cancel_code_index_query", Data: protocol.CommandCancelCodeIndexQueryPayload{Request: request.ID}})
		if cancelled.Request != request.ID {
			t.Fatalf("cancellation answered for %s", cancelled.Request)
		}
		select {
		case response := <-accepted.Completion:
			acceptanceFault(t, "cancelled running query", response.Fault, protocol.ErrorCodeCancelled, "code query was cancelled")
			t.Logf("a running query stopped %s after its cancellation", time.Since(started).Round(time.Millisecond))
		case <-time.After(acceptanceWait):
			t.Fatalf("the cancelled query did not stop within %s", acceptanceWait)
		}
		node.codeIndex.mu.Lock()
		remaining := len(node.codeIndex.running)
		node.codeIndex.mu.Unlock()
		if remaining != 0 {
			t.Fatalf("%d queries stayed registered", remaining)
		}
		// A cancellation that arrives first stops its query on admission.
		early := protocol.NewRequest(node.ID(), protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: worktree, Query: protocol.CodeIndexSearch{Query: "Bulk", Limit: 20}}})
		acceptanceDo[protocol.OutputCodeIndexQueryCancelledPayload](t, node, protocol.Command{Kind: "cancel_code_index_query", Data: protocol.CommandCancelCodeIndexQueryPayload{Request: early.ID}})
		_, failure := acceptanceRequest[protocol.CodeIndexSearchPage](t, node, early, acceptanceWait)
		acceptanceFault(t, "query cancelled early", failure, protocol.ErrorCodeCancelled, "code query was cancelled")
		// The interrupted index completes on later queries: reconcile works
		// within a 15 s budget per query, so this wait is bounded at 60 s.
		finish := time.Now().Add(60 * time.Second)
		for {
			page, failure := acceptanceRequest[protocol.CodeIndexSearchPage](t, node, protocol.NewRequest(node.ID(), protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: worktree, Query: protocol.CodeIndexSearch{Query: "Bulk239Function299", Limit: 20}}}), 30*time.Second)
			if failure != nil {
				t.Fatalf("search after cancellation: %+v", failure)
			}
			if !page.Partial && !page.Truncated {
				if len(page.Symbols) != 1 || page.Symbols[0].Path != "bulk/file239.go" {
					t.Fatalf("the completed index found %+v", codeSymbols(page.Symbols))
				}
				break
			}
			if time.Now().After(finish) {
				t.Fatalf("the index did not complete after a cancellation: %+v", page)
			}
		}

		// Removing the project removes the cache of its worktree.
		if _, err := os.Stat(filepath.Join(cache(worktree), "index.sqlite")); err != nil {
			t.Fatalf("the project has no index cache: %v", err)
		}
		var current protocol.Project
		for _, known := range snapshotFixture(t, node).Projects {
			if known.ID == bulk.ID {
				current = known
			}
		}
		acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "remove_project", Data: protocol.CommandRemoveProjectPayload{Expected: current}})
		if _, err := os.Stat(cache(worktree)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("the removed project kept its index cache: %v", err)
		}
	})

	t.Run("settings", func(t *testing.T) {
		settings := acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "read_code_index_settings"})
		if settings != (protocol.CodeIndexSettings{Revision: 0, Enabled: true}) {
			t.Fatalf("default settings %+v", settings)
		}
		disabled := acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Revision: 0, Enabled: false}, ExpectedRevision: 0}})
		if disabled != (protocol.CodeIndexSettings{Revision: 1, Enabled: false}) {
			t.Fatalf("disabled settings %+v", disabled)
		}
		// The saved enablement applies before the save completes.
		_, failure := acceptanceCall[protocol.CodeIndexSearchPage](t, node, protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: main, Query: protocol.CodeIndexSearch{Query: "Serve", Limit: 20}}})
		acceptanceFault(t, "disabled search", failure, protocol.ErrorCodeNotConfigured, "code index is disabled")
		_, failure = acceptanceCall[protocol.CodeIndexOutline](t, node, protocol.Command{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: main, Path: "cmd/server.go"}})
		acceptanceFault(t, "disabled outline", failure, protocol.ErrorCodeNotConfigured, "code index is disabled")
		name := "helper"
		_, failure = acceptanceCall[protocol.CodeIndexGraphGraphPage](t, node, protocol.Command{Kind: "graph_code", Data: protocol.CommandGraphCodePayload{Worktree: main, Query: protocol.CodeIndexGraph{View: protocol.CodeIndexGraphViewCallers, Name: &name, Paths: []string{}, Depth: 3, Limit: 50}}})
		acceptanceFault(t, "disabled graph", failure, protocol.ErrorCodeNotConfigured, "code index is disabled")
		if _, err := os.Stat(filepath.Join(cache(main), "index.sqlite")); err != nil {
			t.Fatalf("disabling removed the cache: %v", err)
		}
		_, failure = acceptanceCall[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Revision: 0, Enabled: true}, ExpectedRevision: 0}})
		acceptanceFault(t, "stale settings", failure, protocol.ErrorCodeRevisionConflict, "code index settings changed before saving")
		enabled := acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Revision: 1, Enabled: true}, ExpectedRevision: 1}})
		if enabled != (protocol.CodeIndexSettings{Revision: 2, Enabled: true}) {
			t.Fatalf("enabled settings %+v", enabled)
		}
		page, failure := acceptanceCall[protocol.CodeIndexSearchPage](t, node, protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: main, Query: protocol.CodeIndexSearch{Query: "Serve", Limit: 20}}})
		if failure != nil {
			t.Fatalf("search after enabling again: %+v", failure)
		}
		if found := codeSymbols(page.Symbols); len(found) == 0 || found[0].name != "Serve" || page.Partial || page.Truncated {
			t.Fatalf("search after enabling again found %+v", page)
		}
		if read := acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "read_code_index_settings"}); read != enabled {
			t.Fatalf("saved settings read as %+v", read)
		}
	})

	t.Run("worktree removal", func(t *testing.T) {
		destination := filepath.Join(filepath.Dir(root), "feature-worktree")
		created := acceptanceDo[protocol.Worktree](t, node, protocol.Command{Kind: "create_worktree", Data: protocol.CommandCreateWorktreePayload{Project: project.ID, Path: destination, Branch: "feature", Commit: commit}})
		// The new worktree holds the committed fixture, not the later edits.
		if found := codeSymbols(codeSearch(t, node, created.ID, protocol.CodeIndexSearch{Query: "build"}).Symbols); !slices.Equal(found, []codeSymbol{{"lib/index.rs", "build", "function", 4}}) {
			t.Fatalf("the second worktree found %+v", found)
		}
		if _, err := os.Stat(filepath.Join(cache(created.ID), "index.sqlite")); err != nil {
			t.Fatalf("the worktree has no index cache: %v", err)
		}
		acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: created.ID, ExpectedHead: commit, ExpectedBranch: "feature"}})
		// The cache is deleted before the removal completes.
		if _, err := os.Stat(cache(created.ID)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("the removed worktree's index cache remains: %v", err)
		}
		if _, err := os.Stat(filepath.Join(cache(main), "index.sqlite")); err != nil {
			t.Fatalf("removing a worktree removed the main cache: %v", err)
		}
	})

	t.Run("fingerprint", func(t *testing.T) {
		before := codeSearch(t, node, main, protocol.CodeIndexSearch{Limit: 100})
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		stopped = true
		database := filepath.Join(cache(main), "index.sqlite")
		if found := codeFingerprints(t, database, ""); !slices.Equal(found, []string{codeindex.Fingerprint}) {
			t.Fatalf("the index carries fingerprints %v, want %s", found, codeindex.Fingerprint)
		}
		if codeindex.Fingerprint != "546f8c024848e7839e914367cbe7fc6491c37ec2c0219904c3b77ba3a34c405c" {
			t.Fatalf("the engine fingerprint is %s", codeindex.Fingerprint)
		}
		// Another engine's rows: same files and hashes, other rules.
		foreign := codeindex.Hash("code-index-v1:another-engine")
		if found := codeFingerprints(t, database, foreign); !slices.Equal(found, []string{foreign}) {
			t.Fatalf("the index was not rewritten: %v", found)
		}
		second := acceptanceNode(t, profile, nil, Options{})
		after := codeSearch(t, second, main, protocol.CodeIndexSearch{Limit: 100})
		if !slices.Equal(codeSymbols(after.Symbols), codeSymbols(before.Symbols)) || after.IndexRevision <= before.IndexRevision {
			t.Fatalf("a foreign fingerprint was not rebuilt: revision %d after %d, symbols %+v", after.IndexRevision, before.IndexRevision, codeSymbols(after.Symbols))
		}
		unchanged := codeSearch(t, second, main, protocol.CodeIndexSearch{Limit: 100})
		if unchanged.IndexRevision != after.IndexRevision {
			t.Fatalf("a current index was rebuilt again: revision %d after %d", unchanged.IndexRevision, after.IndexRevision)
		}
		if err := second.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		if found := codeFingerprints(t, database, ""); !slices.Equal(found, []string{codeindex.Fingerprint}) {
			t.Fatalf("the rebuilt index carries fingerprints %v", found)
		}
		// The engine's own rules change: the next start rebuilds as well.
		original := codeindex.Fingerprint
		codeindex.Fingerprint = codeindex.Hash("code-index-v1:gotreesitter:next-rules")
		defer func() { codeindex.Fingerprint = original }()
		third := acceptanceNode(t, profile, nil, Options{})
		defer third.Stop(context.Background())
		rebuilt := codeSearch(t, third, main, protocol.CodeIndexSearch{Limit: 100})
		if !slices.Equal(codeSymbols(rebuilt.Symbols), codeSymbols(before.Symbols)) || rebuilt.IndexRevision <= after.IndexRevision {
			t.Fatalf("changed rules were not rebuilt: revision %d after %d", rebuilt.IndexRevision, after.IndexRevision)
		}
		if err := third.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		if found := codeFingerprints(t, database, ""); !slices.Equal(found, []string{codeindex.Fingerprint}) {
			t.Fatalf("the index carries fingerprints %v after a rule change", found)
		}
	})
}

// TestCodeAgentTools runs one agent turn in a default profile and calls every
// tool of the bundled code package.
func TestCodeAgentTools(t *testing.T) {
	search, outline := operations.Alias("code", "code_search"), operations.Alias("code", "code_outline")
	codeContext, impact, ranked := operations.Alias("code", "code_context"), operations.Alias("code", "code_impact"), operations.Alias("code", "code_map")
	tools := []string{search, outline, codeContext, impact, ranked}
	script := &acceptanceScript{steps: []acceptanceStep{
		{"discover", "tool_search", acceptanceArguments(map[string]any{"query": "select:" + strings.Join(tools, ",")})},
		{"search", search, acceptanceArguments(map[string]any{"query": "Serve"})},
		{"outline", outline, acceptanceArguments(map[string]any{"path": "cmd/server.go"})},
		{"context", codeContext, acceptanceArguments(map[string]any{"name": "helper"})},
		{"impact", impact, acceptanceArguments(map[string]any{"name": "helper"})},
		{"map", ranked, acceptanceArguments(map[string]any{})},
		{id: "answer"},
	}}
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), script, Options{})
	defer node.Stop(context.Background())
	root, _ := codeProject(t, codeFixture)
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Code fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	session := acceptanceSession(t, node, project.ID, protocol.PermissionAsk)
	// No approval is expected: every code tool is read-only.
	frozen, results := acceptanceTurn(t, node, session, "Explain how the server starts", acceptanceWait, nil)
	if !slices.ContainsFunc(frozen, func(reference protocol.PluginReference) bool { return reference.Name == "code" }) {
		t.Fatalf("the default profile did not freeze the code package: %+v", frozen)
	}
	requests := script.requests()
	if len(requests) != len(script.steps) {
		t.Fatalf("the turn made %d model requests, want %d", len(requests), len(script.steps))
	}
	t.Logf("discovery answered: %s", results["discover"].Result.Value)
	for _, tool := range tools {
		if !slices.Contains(requests[1], tool) {
			t.Errorf("%s was not offered after discovery; offered %v", tool, requests[1])
		}
	}

	found := acceptanceResult(t, results, "search", search)
	t.Logf("code_search: %s", results["search"].Result.Value)
	data, _ := found["data"].(map[string]any)
	symbols, _ := data["symbols"].([]any)
	if found["kind"] != "code_search" || len(symbols) == 0 {
		t.Fatalf("code_search returned %v", found)
	}
	first, _ := symbols[0].(map[string]any)
	// Serve is declared on the thirteenth line and ends on the fifteenth.
	if first["path"] != "cmd/server.go" || first["name"] != "Serve" || first["language"] != "go" || first["start_line"] != float64(13) || first["end_line"] != float64(15) {
		t.Fatalf("code_search placed Serve at %v", first)
	}
	if _, ranged := first["range"]; ranged {
		t.Fatalf("code_search exposed byte ranges: %v", first)
	}
	if _, visited := data["visited"]; visited {
		t.Fatalf("code_search exposed the visit count: %v", data)
	}

	listed := acceptanceResult(t, results, "outline", outline)
	t.Logf("code_outline: %s", results["outline"].Result.Value)
	data, _ = listed["data"].(map[string]any)
	lines := map[string][2]float64{}
	for _, item := range data["symbols"].([]any) {
		symbol := item.(map[string]any)
		lines[symbol["name"].(string)] = [2]float64{symbol["start_line"].(float64), symbol["end_line"].(float64)}
		if _, located := symbol["path"]; located {
			t.Fatalf("code_outline repeated the path per symbol: %v", symbol)
		}
	}
	want := map[string][2]float64{"Config": {4, 6}, "main": {8, 10}, "Serve": {13, 15}, "helper": {17, 19}}
	if listed["kind"] != "code_outline" || data["path"] != "cmd/server.go" || data["language"] != "go" || fmt.Sprint(lines) != fmt.Sprint(want) {
		t.Fatalf("code_outline returned lines %v, want %v: %v", lines, want, listed)
	}

	for _, call := range []string{"context", "impact", "map"} {
		if response := results[call]; response != nil {
			t.Logf("%s: status %v: %s", call, response.Status, response.Result.Value)
		}
	}
	related := acceptanceResult(t, results, "context", codeContext)
	if related["isError"] == true {
		t.Fatalf("code_context failed: %v", related)
	}
	snippets, _ := related["snippets"].([]any)
	callers, _ := related["callers"].([]any)
	contextSymbols, _ := related["symbols"].([]any)
	if len(contextSymbols) != 1 || len(snippets) != 1 || len(callers) != 1 || related["graph_unavailable"] != nil {
		t.Fatalf("code_context returned %v", related)
	}
	if symbol := contextSymbols[0].(map[string]any); symbol["name"] != "helper" || symbol["path"] != "cmd/server.go" || symbol["start_line"] != float64(17) || symbol["end_line"] != float64(19) {
		t.Fatalf("code_context placed helper at %v", symbol)
	}
	if snippet := snippets[0].(map[string]any); snippet["path"] != "cmd/server.go" || !strings.Contains(snippet["text"].(string), "func helper(address string) {") || snippet["start_line"] != float64(15) {
		t.Fatalf("code_context read snippet %v", snippet)
	}
	if caller := callers[0].(map[string]any); caller["path"] != "cmd/server.go" || caller["scope"] != "Serve" || caller["line"] != float64(14) {
		t.Fatalf("code_context found caller %v", caller)
	}

	affected := acceptanceResult(t, results, "impact", impact)
	if affected["isError"] == true {
		t.Fatalf("code_impact failed: %v", affected)
	}
	targets, _ := affected["targets"].([]any)
	files, _ := affected["files"].([]any)
	if len(targets) != 1 || len(files) != 1 {
		t.Fatalf("code_impact returned %v", affected)
	}
	if target := targets[0].(map[string]any); target["name"] != "helper" || target["start_line"] != float64(17) {
		t.Fatalf("code_impact target %v", target)
	}
	file := files[0].(map[string]any)
	names := []string{}
	for _, item := range file["symbols"].([]any) {
		names = append(names, item.(map[string]any)["name"].(string))
	}
	slices.Sort(names)
	// Serve calls helper, and main calls Serve.
	if file["path"] != "cmd/server.go" || !slices.Equal(names, []string{"Serve", "main"}) {
		t.Fatalf("code_impact affected %v in %v", names, file)
	}

	overview := acceptanceResult(t, results, "map", ranked)
	if overview["isError"] == true {
		t.Fatalf("code_map failed: %v", overview)
	}
	paths := []string{}
	for _, item := range overview["files"].([]any) {
		paths = append(paths, item.(map[string]any)["path"].(string))
	}
	slices.Sort(paths)
	if !slices.Equal(paths, []string{"app/service.py", "cmd/server.go", "lib/index.rs", "settings.json", "src/page.ts"}) || overview["total"] != float64(5) {
		t.Fatalf("code_map ranked %v: %v", paths, overview)
	}
}
