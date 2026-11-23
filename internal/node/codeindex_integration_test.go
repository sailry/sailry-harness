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
	"testing"
	"time"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/modules/code-index/node"
)

func requireCodeIndex(t *testing.T) {
	t.Helper()
	var parser codeindex.Parser
	if _, err := parser.ParseOutline(t.Context(), "main.go", "package main\n"); err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) && fault.Code == protocol.ErrorCodeUnavailable {
			t.Skip(fault.Message)
		}
		t.Fatal(err)
	}
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
	requireCodeIndex(t)
	profile := filepath.Join(t.TempDir(), "profile")
	node := acceptanceNode(t, profile, nil, Options{CodeIndex: codeindex.NewService})
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
	settings := func() protocol.CodeIndexSettings {
		return acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "read_code_index_settings"})
	}
	configure := func(projects ...protocol.ProjectID) {
		current := settings()
		current.Persistent = nil // The old client's request shape.
		current.PersistentProjects = new(append([]protocol.ProjectID{}, projects...))
		acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: current, ExpectedRevision: current.Revision}})
	}
	if found := settings(); !found.Enabled || found.Revision != 0 || found.Persistent == nil || *found.Persistent || found.PersistentProjects == nil || len(*found.PersistentProjects) != 0 {
		t.Fatalf("default settings %+v", found)
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
		if data.Language == nil || *data.Language != "json" || data.Status != codeindex.StatusUnsupported || len(data.Symbols) != 0 {
			t.Fatalf("outline of settings.json: %+v", data)
		}
		_, failure := acceptanceCall[protocol.CodeIndexOutline](t, node, protocol.Command{Kind: "outline_code", Data: protocol.CommandOutlineCodePayload{Worktree: main, Path: "../outside.go"}})
		if failure == nil {
			t.Fatal("an outline outside the worktree was answered")
		}
	})

	t.Run("on demand", func(t *testing.T) {
		found := codeSearch(t, node, main, protocol.CodeIndexSearch{Query: "Serve"})
		if len(found.Symbols) == 0 || found.Symbols[0].Name != "Serve" || found.IndexRevision != 0 || found.Partial {
			t.Fatalf("on-demand search returned %+v", found)
		}
		// A caller in another file is discovered without building a worktree index.
		path := filepath.Join(root, "cmd", "bridge.go")
		if err := os.WriteFile(path, []byte("package main\nfunc Bridge() { helper(\"bridge\") }\n"), 0o600); err != nil {
			t.Fatal(err)
		}
		defer os.Remove(path)
		graph := codeGraph(t, node, main, protocol.CodeIndexGraphViewCallers, "helper")
		if graph.IndexRevision != 0 || !slices.Contains(codeCallers(graph), "cmd/bridge.go:Bridge") {
			t.Fatalf("on-demand graph returned %+v", graph)
		}
		if _, err := os.Stat(filepath.Join(cache(main), "index.turso")); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("default commands persisted a code index: %v", err)
		}
	})
	configure(project.ID)

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
		if !slices.Equal(paths, []string{"app/service.py", "cmd/server.go", "lib/index.rs", "src/page.ts"}) || ranked.Total != 4 {
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
		configure(project.ID, bulk.ID)
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
		if _, err := os.Stat(filepath.Join(cache(worktree), "index.turso")); err != nil {
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
		configure(project.ID)
		current := settings()
		disabled := acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Revision: current.Revision, Enabled: false}, ExpectedRevision: current.Revision}})
		if disabled.Enabled || disabled.Revision != current.Revision+1 || disabled.PersistentProjects == nil || !slices.Equal(*disabled.PersistentProjects, []protocol.ProjectID{project.ID}) {
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
		if _, err := os.Stat(filepath.Join(cache(main), "index.turso")); err != nil {
			t.Fatalf("disabling removed the cache: %v", err)
		}
		_, failure = acceptanceCall[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Revision: 0, Enabled: true}, ExpectedRevision: 0}})
		acceptanceFault(t, "stale settings", failure, protocol.ErrorCodeRevisionConflict, "code index settings changed before saving")
		enabled := acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{Settings: protocol.CodeIndexSettings{Revision: disabled.Revision, Enabled: true}, ExpectedRevision: disabled.Revision}})
		if !enabled.Enabled || enabled.Revision != disabled.Revision+1 || enabled.PersistentProjects == nil || !slices.Equal(*enabled.PersistentProjects, []protocol.ProjectID{project.ID}) {
			t.Fatalf("enabled settings %+v", enabled)
		}
		page, failure := acceptanceCall[protocol.CodeIndexSearchPage](t, node, protocol.Command{Kind: "search_code", Data: protocol.CommandSearchCodePayload{Worktree: main, Query: protocol.CodeIndexSearch{Query: "Serve", Limit: 20}}})
		if failure != nil {
			t.Fatalf("search after enabling again: %+v", failure)
		}
		if found := codeSymbols(page.Symbols); len(found) == 0 || found[0].name != "Serve" || page.Partial || page.Truncated {
			t.Fatalf("search after enabling again found %+v", page)
		}
		if read := settings(); read.Enabled != enabled.Enabled || read.Revision != enabled.Revision || read.PersistentProjects == nil || !slices.Equal(*read.PersistentProjects, *enabled.PersistentProjects) {
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
		if _, err := os.Stat(filepath.Join(cache(created.ID), "index.turso")); err != nil {
			t.Fatalf("the worktree has no index cache: %v", err)
		}
		acceptanceDo[json.RawMessage](t, node, protocol.Command{Kind: "remove_worktree", Data: protocol.CommandRemoveWorktreePayload{Worktree: created.ID, ExpectedHead: commit, ExpectedBranch: "feature"}})
		// The cache is deleted before the removal completes.
		if _, err := os.Stat(cache(created.ID)); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("the removed worktree's index cache remains: %v", err)
		}
		if _, err := os.Stat(filepath.Join(cache(main), "index.turso")); err != nil {
			t.Fatalf("removing a worktree removed the main cache: %v", err)
		}
	})

	t.Run("host persistence supersedes legacy choices", func(t *testing.T) {
		otherRoot, _ := codeProject(t, codeFixture)
		other := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
			Name: "Another code fixture", Path: otherRoot, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
		}})
		otherMain := codeMain(t, node, other.ID)
		current := settings()
		if current.Persistent == nil || !*current.Persistent || current.PersistentProjects == nil || !slices.Equal(*current.PersistentProjects, []protocol.ProjectID{project.ID}) {
			t.Fatalf("legacy opt-in settings %+v", current)
		}
		query := protocol.CodeIndexSearch{Query: "Serve"}
		if page := codeSearch(t, node, otherMain, query); page.IndexRevision != 0 || len(page.Symbols) == 0 {
			t.Fatalf("legacy choices affected a second project: %+v", page)
		}
		for _, enabled := range []bool{false, true} {
			current = acceptanceDo[protocol.CodeIndexSettings](t, node, protocol.Command{Kind: "save_code_index_settings", Data: protocol.CommandSaveCodeIndexSettingsPayload{
				Settings: protocol.CodeIndexSettings{Revision: current.Revision, Enabled: true, Persistent: new(enabled)}, ExpectedRevision: current.Revision,
			}})
			if current.Persistent == nil || *current.Persistent != enabled || current.PersistentProjects == nil || !slices.Equal(*current.PersistentProjects, []protocol.ProjectID{project.ID}) {
				t.Fatalf("host persistence settings %+v", current)
			}
			for _, worktree := range []protocol.WorktreeID{main, otherMain} {
				page := codeSearch(t, node, worktree, query)
				if (page.IndexRevision != 0) != enabled || len(page.Symbols) == 0 {
					t.Fatalf("host persistence %v returned %+v", enabled, page)
				}
			}
		}
	})

	t.Run("fingerprint", func(t *testing.T) {
		before := codeSearch(t, node, main, protocol.CodeIndexSearch{Limit: 100})
		if err := node.Stop(context.Background()); err != nil {
			t.Fatal(err)
		}
		stopped = true
		database := filepath.Join(cache(main), "index.turso")
		if found := codeFingerprints(t, database, ""); !slices.Equal(found, []string{codeindex.Fingerprint}) {
			t.Fatalf("the index carries fingerprints %v, want %s", found, codeindex.Fingerprint)
		}
		if codeindex.Fingerprint != codeindex.Hash("code-index-v2:gotreesitter:v0.57.0:embedded-1:source-1:references-2:budget-64m") {
			t.Fatalf("the engine fingerprint is %s", codeindex.Fingerprint)
		}
		// Another engine's rows: same files and hashes, other rules.
		foreign := codeindex.Hash("code-index-v1:another-engine")
		if found := codeFingerprints(t, database, foreign); !slices.Equal(found, []string{foreign}) {
			t.Fatalf("the index was not rewritten: %v", found)
		}
		second := acceptanceNode(t, profile, nil, Options{CodeIndex: codeindex.NewService})
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
		third := acceptanceNode(t, profile, nil, Options{CodeIndex: codeindex.NewService})
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
// native code-index module tool.
func TestCodeAgentTools(t *testing.T) {
	requireCodeIndex(t)
	search, outline := "code_search", "code_outline"
	codeContext, impact, ranked := "code_context", "code_impact", "code_map"
	pipeline := "code_pipeline"
	tools := []string{search, outline, codeContext, impact, ranked, pipeline}
	script := &acceptanceScript{steps: []acceptanceStep{
		{"discover", "tool_search", acceptanceArguments(map[string]any{"query": "select:" + strings.Join(tools, ",")})},
		{"search", search, acceptanceArguments(map[string]any{"query": "Serve"})},
		{"outline", outline, acceptanceArguments(map[string]any{"path": "cmd/server.go"})},
		{"context", codeContext, acceptanceArguments(map[string]any{"name": "helper"})},
		{"impact", impact, acceptanceArguments(map[string]any{"name": "helper"})},
		{"map", ranked, acceptanceArguments(map[string]any{})},
		{"baseline_search", search, acceptanceArguments(map[string]any{"name": "helper", "limit": 8})},
		{"baseline_read", "read_file", acceptanceArguments(map[string]any{"path": "cmd/server.go", "offset": 16, "limit": 1})},
		{"pipeline", pipeline, acceptanceArguments(json.RawMessage(`{"steps":[{"operation":"code.search","arguments":{"name":"helper"}},{"operation":"files.read_lines","arguments":{"limit":1},"from_previous":{"path":"/symbols/0/path","offset":"/symbols/0/range/start_line","expected_revision":"/symbols/0/file_revision"}}],"select":"/lines/0/text"}`))},
		{id: "answer"},
	}}
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), script, Options{Assembly: hostassembly.Harness()})
	defer node.Stop(context.Background())
	root, _ := codeProject(t, codeFixture)
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Code fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	session := acceptanceSession(t, node, project.ID, protocol.PermissionAsk)
	// No approval is expected: every code tool is read-only.
	frozen, results := acceptanceTurn(t, node, session, "Explain how the server starts", acceptanceWait, nil)
	if slices.ContainsFunc(frozen, func(reference protocol.PluginReference) bool { return reference.Name == "code" }) {
		t.Fatalf("the default profile should not freeze the retired code package: %+v", frozen)
	}
	requests := script.requests()
	if len(requests) != len(script.steps) {
		t.Fatalf("the turn made %d model requests, want %d", len(requests), len(script.steps))
	}
	t.Logf("discovery answered: %s", results["discover"].Result.Value)
	for _, tool := range tools {
		// The model reads a package tool under its declared name.
		if !slices.Contains(requests[1], declaredToolName(tool)) {
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

	selected := acceptanceResult(t, results, "pipeline", pipeline)
	if value, ok := selected["value"].(string); !ok || strings.TrimSpace(value) != "func helper(address string) {" || len(selected) > 2 {
		t.Fatalf("code_pipeline did not isolate the selected source line: %v", selected)
	}
	baseline := acceptanceResult(t, results, "baseline_read", "read_file")
	baselineText := baseline["data"].(map[string]any)["lines"].([]any)[0].(map[string]any)["text"]
	if selected["value"] != baselineText {
		t.Fatalf("pipeline and ordinary read returned different source lines: %v and %v", selected, baseline)
	}
	t.Logf("Same lookup, tool result bytes: separate search and read %d, pipeline %d", len(results["baseline_search"].Result.Value)+len(results["baseline_read"].Result.Value), len(results["pipeline"].Result.Value))

	overview := acceptanceResult(t, results, "map", ranked)
	if overview["isError"] == true {
		t.Fatalf("code_map failed: %v", overview)
	}
	paths := []string{}
	for _, item := range overview["files"].([]any) {
		paths = append(paths, item.(map[string]any)["path"].(string))
	}
	slices.Sort(paths)
	if !slices.Equal(paths, []string{"app/service.py", "cmd/server.go", "lib/index.rs", "src/page.ts"}) || overview["total"] != float64(4) {
		t.Fatalf("code_map ranked %v: %v", paths, overview)
	}
}

func TestCodeAgentToolsWithoutIndex(t *testing.T) {
	pipeline := "code_pipeline"
	read := "read_file"
	requested := []string{pipeline, read, "code_search", "code_outline", "code_context", "code_impact", "code_map"}
	script := &acceptanceScript{steps: []acceptanceStep{
		{"discover", "tool_search", acceptanceArguments(map[string]any{"query": "select:" + strings.Join(requested, ","), "limit": len(requested)})},
		{"pipeline", pipeline, acceptanceArguments(json.RawMessage(`{"steps":[{"operation":"files.find","arguments":{"globs":["cmd/server.go"]}},{"operation":"files.read_lines","arguments":{"limit":1},"from_previous":{"path":"/paths/0"}}],"select":"/lines/0/text"}`))},
		{id: "answer"},
	}}
	node := acceptanceNode(t, filepath.Join(t.TempDir(), "profile"), script, Options{Assembly: moduleapi.Assembly{Modules: hostassembly.Harness().Modules}})
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	root, _ := codeProject(t, codeFixture)
	project := acceptanceDo[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Code fixture", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}})
	session := acceptanceSession(t, node, project.ID, protocol.PermissionAsk)
	frozen, results := acceptanceTurn(t, node, session, "Read the first line of the server source", acceptanceWait, nil)
	if slices.ContainsFunc(frozen, func(reference protocol.PluginReference) bool { return reference.Name == "code" }) {
		t.Fatalf("the default profile should not freeze the retired code package: %+v", frozen)
	}
	if !slices.ContainsFunc(frozen, func(reference protocol.PluginReference) bool { return reference.Name == "files" }) {
		t.Fatalf("the default profile did not freeze the files package: %+v", frozen)
	}

	requests := script.requests()
	if len(requests) != len(script.steps) {
		t.Fatalf("the turn made %d model requests, want %d", len(requests), len(script.steps))
	}
	var initial []string
	for _, tool := range requests[0] {
		name := declaredToolName(tool)
		if slices.ContainsFunc(requested, func(alias string) bool { return declaredToolName(alias) == name }) {
			initial = append(initial, name)
		}
	}
	slices.Sort(initial)
	if want := []string{"code_pipeline", "read_file"}; !slices.Equal(initial, want) {
		t.Fatalf("initial request without an index offered %v among the requested tools, want %v", initial, want)
	}
	discovered := acceptanceResult(t, results, "discover", "tool_search")
	if tools, ok := discovered["tools"].([]any); !ok || len(tools) != 0 {
		t.Fatalf("tool discovery without an index returned %v, want an empty tools array", discovered)
	}
	if result := acceptanceResult(t, results, "pipeline", pipeline); strings.TrimSpace(fmt.Sprint(result["value"])) != "package main" {
		t.Fatalf("file-only pipeline returned %v", result)
	}
}
