package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/localmodels"
	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The real indexing worker must restore each stored source's user rather than
// reading every source as the Node's local maintenance principal.
func TestKnowledgeUserAndProjectStorage(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	var encoded atomic.Int64
	var node *Node
	start := func() {
		t.Helper()
		started, err := StartWithOptions(t.Context(), profile, Options{Environment: &environment, knowledge: func(options *knowledgeservice.Options) {
			model := localmodels.DefaultEmbedding
			options.Choice = func() protocol.ModelsLocalChoice { return protocol.ModelsLocalChoice{Enabled: true, Model: &model} }
			options.Dimension = func(id string) (int, string, bool) { return 2, "users-fixture", id == model }
			options.Installed = func() (map[string]bool, error) { return map[string]bool{model: true}, nil }
			options.Offsets = func(context.Context, string, protocol.ModelsLocalChoice, string) ([][2]int, error) { return nil, nil }
			options.Encode = func(_ context.Context, _ string, _ protocol.ModelsLocalChoice, texts []string, query bool) ([][]float32, error) {
				if !query {
					encoded.Add(int64(len(texts)))
				}
				vectors := make([][]float32, len(texts))
				for index, text := range texts {
					vectors[index] = []float32{0, 1}
					if strings.Contains(text, "alpha") {
						vectors[index] = []float32{1, 0}
					}
				}
				return vectors, nil
			}
		}})
		if err != nil {
			t.Fatal(err)
		}
		node = started
	}
	start()
	defer func() {
		if node != nil {
			_ = node.Stop(context.Background())
		}
	}()
	ctx := t.Context()
	var first, second protocol.User
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		first, err = store.LocalUser(ctx, db)
		if err != nil {
			return err
		}
		second, err = store.CreateUser(ctx, db, "Knowledge user")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := node.InitializeUserPlugins(ctx, second); err != nil {
		t.Fatal(err)
	}
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"knowledge-users","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write","knowledge.read","knowledge.write"],"storage":{"collections":[{"scope":"node","prefix":"entry/","schema":{"type":"object","properties":{"text":{"type":"string"}}},"index":{"semantic":true}},{"scope":"project","prefix":"entry/","schema":{"type":"object","properties":{"text":{"type":"string"}}},"index":{"semantic":true}}]}}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := node.plugins.manager.Install(ctx, source, "", "knowledge-users")
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("package %v: %#v", err, info.Issues)
	}
	references := map[protocol.UserID]protocol.PluginReference{}
	for _, user := range []protocol.User{first, second} {
		owner := store.WithUser(ctx, user)
		if err := node.profile.Store.Node.Write(owner, func(db *sql.Conn) error {
			installed, err := store.InstallPlugin(owner, db, info, 0, true)
			references[user.ID] = plugins.PackageReference(installed)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	call := func(user protocol.User, scope *protocol.PluginContext, command protocol.Command) (protocol.Output, *protocol.Fault) {
		t.Helper()
		request := protocol.NewRequest(node.ID(), command)
		request.Plugin = scope
		accepted, err := node.Dispatch(store.WithUser(ctx, user), node.ID(), request)
		if err != nil {
			var failure *protocol.Fault
			if !errors.As(err, &failure) {
				t.Fatal(err)
			}
			return protocol.Output{}, failure
		}
		result := <-accepted.Completion
		return result.Output, result.Fault
	}
	must := func(user protocol.User, scope *protocol.PluginContext, command protocol.Command) protocol.Output {
		t.Helper()
		output, failure := call(user, scope, command)
		if failure != nil {
			t.Fatalf("%s: %v", command.Kind, failure)
		}
		return output
	}
	registered := must(first, nil, protocol.Command{Kind: "register_project", Data: &protocol.CommandRegisterProjectPayload{Name: "Shared knowledge", Path: t.TempDir(), Visibility: protocol.ProjectVisibilityShared}})
	project, ok := dispatchPayload[protocol.Project](registered.Data)
	if !ok {
		t.Fatalf("project output: %#v", registered)
	}
	var worktree protocol.WorktreeID
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var id string
		if err := db.QueryRowContext(ctx, "SELECT id FROM worktrees WHERE project=?1 AND main=1", project.ID.String()).Scan(&id); err != nil {
			return err
		}
		var err error
		worktree, err = protocol.ParseWorktreeID(id)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	scope := func(user protocol.User, project bool) *protocol.PluginContext {
		value := protocol.DefaultPluginContext()
		value.Package = references[user.ID]
		if project {
			value.Worktree = &worktree
		}
		return &value
	}
	write := func(user protocol.User, project bool, text string) {
		t.Helper()
		storage := protocol.PluginStorageScopeNode
		if project {
			storage = protocol.PluginStorageScopeProject
		}
		body, _ := json.Marshal(map[string]string{"text": text})
		must(user, scope(user, project), protocol.Command{Kind: "write_plugin_value", Data: &protocol.CommandWritePluginValuePayload{Key: "entry/raw", Value: body, Scope: storage}})
		must(user, scope(user, project), protocol.Command{Kind: "write_indexed_plugin_value", Data: &protocol.CommandWriteIndexedPluginValuePayload{Key: "entry/record", Value: body, Scope: storage, Index: protocol.PluginStorageIndex{Fields: [2]string{text, ""}, Source: &protocol.PluginStorageSource{Key: "entry/raw", Pointer: "/text"}}}})
	}
	write(first, false, "alpha personal source")
	write(second, false, "beta personal source")
	write(first, true, "sharedprojectsource")
	ready := func() {
		t.Helper()
		for deadline := time.Now().Add(30 * time.Second); ; time.Sleep(20 * time.Millisecond) {
			count := 0
			err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				catalog, err := store.ReadKnowledgeCatalog(ctx, db)
				if err != nil {
					return err
				}
				for _, stored := range catalog {
					if stored.Capture.Package != nil && stored.Capture.Package.Name == "knowledge-users" && stored.Collection.Progress.Status == protocol.KnowledgeStatusReady && stored.Collection.Progress.Sources == 1 {
						count++
					}
				}
				return nil
			})
			if err != nil {
				t.Fatal(err)
			}
			if count == 3 {
				return
			}
			if time.Now().After(deadline) {
				t.Fatalf("worker indexed %d of three user/project sources", count)
			}
		}
	}
	ready()
	knowledgeHit := func(user protocol.User, text string) protocol.KnowledgeHit {
		t.Helper()
		query := protocol.DefaultKnowledgeQuery()
		query.Query = text
		query.Mode = protocol.KnowledgeModeKeyword
		query.Limit = 10
		output := must(user, nil, protocol.Command{Kind: "query_knowledge", Data: &query})
		page, ok := dispatchPayload[protocol.KnowledgePage](output.Data)
		if !ok || len(page.Hits) != 1 || !strings.Contains(page.Hits[0].Text, text) {
			t.Fatalf("index source %q: %#v", text, output)
		}
		return page.Hits[0]
	}
	personal := knowledgeHit(second, "beta")
	citationOutput := must(second, nil, protocol.Command{Kind: "read_knowledge_citation", Data: &protocol.KnowledgeCitationRequest{Collection: personal.Citation.Collection, Chunk: personal.Citation.Chunk, Revision: personal.Citation.Revision, Limit: 4096}})
	citation, ok := dispatchPayload[protocol.KnowledgeCitationPage](citationOutput.Data)
	if !ok || citation.Text != "beta personal source" {
		t.Fatalf("foreign source capture: %#v", citationOutput)
	}
	knowledgeHit(first, "alpha")
	projectHit := knowledgeHit(second, "sharedprojectsource")
	if encoded.Load() < 3 {
		t.Fatalf("worker embedded only %d source texts", encoded.Load())
	}
	search := func(user protocol.User, project bool, text string) protocol.PluginStorageSearchPage {
		t.Helper()
		query := protocol.DefaultPluginStorageSearch()
		query.Query = &text
		query.Weights = [2]uint16{2, 1}
		query.Limit = 10
		query.Mode = protocol.KnowledgeModeHybrid
		if project {
			query.Scope = protocol.PluginStorageScopeProject
		}
		output := must(user, scope(user, project), protocol.Command{Kind: "search_plugin_values", Data: &query})
		page, ok := dispatchPayload[protocol.PluginStorageSearchPage](output.Data)
		if !ok {
			t.Fatalf("search output: %#v", output)
		}
		return page
	}
	assertValue := func(user protocol.User, project bool, text string) {
		t.Helper()
		page := search(user, project, text)
		if len(page.Entries) != 1 || !strings.Contains(string(page.Entries[0].Value), text) {
			t.Fatalf("user/project search returned %#v for %q", page, text)
		}
	}
	assertValue(first, false, "alpha personal source")
	assertValue(second, false, "beta personal source")
	assertValue(second, true, "sharedprojectsource")
	if err := node.Stop(context.Background()); err != nil {
		t.Fatal(err)
	}
	node = nil
	start()
	ready()
	knowledgeHit(second, "beta")
	knowledgeHit(first, "alpha")
	projectHit = knowledgeHit(second, "sharedprojectsource")
	assertValue(first, false, "alpha personal source")
	assertValue(second, false, "beta personal source")
	assertValue(second, true, "sharedprojectsource")
	private := protocol.ProjectVisibilityPrivate
	must(first, nil, protocol.Command{Kind: "update_project", Data: &protocol.CommandUpdateProjectPayload{Expected: project, Name: project.Name, Path: project.Path, Appearance: project.Appearance, Visibility: &private}})
	_, failure := call(second, scope(second, true), protocol.Command{Kind: "read_plugin_value", Data: &protocol.CommandReadPluginValuePayload{Key: "entry/record", Scope: protocol.PluginStorageScopeProject}})
	if failure == nil {
		t.Fatal("project storage survived visibility revocation")
	}
	_, failure = call(second, nil, protocol.Command{Kind: "read_knowledge_citation", Data: &protocol.KnowledgeCitationRequest{Collection: projectHit.Citation.Collection, Chunk: projectHit.Citation.Chunk, Revision: projectHit.Citation.Revision, Limit: 4096}})
	if failure == nil {
		t.Fatal("project citation survived visibility revocation")
	}
	query := protocol.DefaultKnowledgeQuery()
	query.Query = "sharedprojectsource"
	query.Mode = protocol.KnowledgeModeKeyword
	query.Limit = 10
	output := must(second, nil, protocol.Command{Kind: "query_knowledge", Data: &query})
	page, ok := dispatchPayload[protocol.KnowledgePage](output.Data)
	if !ok {
		t.Fatalf("knowledge output: %#v", output)
	}
	if len(page.Hits) != 0 {
		t.Fatalf("revoked project index remained visible: %#v", page.Hits)
	}
	assertValue(second, false, "beta personal source")

	// Merge into an already indexed namespace without changing its package.
	// The colliding record stays with the target; the new source must appear
	// through the running worker without another storage write or a restart.
	imported := json.RawMessage(`{"text":"imported personal source"}`)
	must(second, scope(second, false), protocol.Command{Kind: "write_plugin_value", Data: &protocol.CommandWritePluginValuePayload{Key: "entry/imported/raw", Value: imported, Scope: protocol.PluginStorageScopeNode}})
	must(second, scope(second, false), protocol.Command{Kind: "write_indexed_plugin_value", Data: &protocol.CommandWriteIndexedPluginValuePayload{
		Key: "entry/imported/record", Value: imported, Scope: protocol.PluginStorageScopeNode,
		Index: protocol.PluginStorageIndex{Fields: [2]string{"imported personal source", ""}, Source: &protocol.PluginStorageSource{Key: "entry/imported/raw", Pointer: "/text"}},
	}})
	personalCollection := func(user protocol.User) store.KnowledgeStored {
		t.Helper()
		var found *store.KnowledgeStored
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			catalog, err := store.ReadKnowledgeCatalog(ctx, db)
			if err != nil {
				return err
			}
			for _, stored := range catalog {
				if stored.Capture.Package != nil && stored.Capture.Package.Name == "knowledge-users" && stored.Capture.Space == "user:"+user.ID.String() {
					found = new(stored)
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		if found == nil {
			t.Fatalf("personal collection missing for %s", user.ID)
		}
		return *found
	}
	waitPersonal := func(user protocol.User, sources uint64) store.KnowledgeStored {
		t.Helper()
		for deadline := time.Now().Add(30 * time.Second); ; time.Sleep(20 * time.Millisecond) {
			stored := personalCollection(user)
			if stored.Collection.Progress.Status == protocol.KnowledgeStatusReady && stored.Collection.Progress.Sources == sources {
				return stored
			}
			if time.Now().After(deadline) {
				t.Fatalf("personal collection after merge: %+v", stored.Collection.Progress)
			}
		}
	}
	waitPersonal(second, 2)
	before := waitPersonal(first, 1)
	// Construct a retained pre-v2 merge directly. New identity-merge commands
	// are retired, while its captured storage and derived indexes remain valid.
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := store.MergeIdentityUsers(store.WithUser(ctx, first), db, second.ID, first.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	node.knowledge.Invalidate()
	merged := personalCollection(first)
	if merged.Collection.ID != before.Collection.ID || merged.Collection.Progress.Generation <= before.Collection.Progress.Generation {
		t.Fatalf("merge retained a stale target generation: before=%+v, after=%+v", before.Collection, merged.Collection)
	}
	waitPersonal(first, 2)
	knowledgeHit(first, "imported")
	knowledgeHit(first, "alpha")
	mergedValues := search(first, false, "alpha personal source")
	foundTarget := false
	for _, entry := range mergedValues.Entries {
		if entry.Key == "entry/record" {
			foundTarget = true
			if string(entry.Value) != `{"text":"alpha personal source"}` {
				t.Fatalf("merge replaced the target storage value: %s", entry.Value)
			}
		}
	}
	if !foundTarget {
		t.Fatal("merge lost the target storage value")
	}
}
