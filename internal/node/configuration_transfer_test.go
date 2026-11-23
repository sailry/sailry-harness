package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationImportFollowsPluginKnowledge(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	passes := make(chan []store.KnowledgeStored)
	node := systemNodeFixture(t, Options{knowledge: func(options *knowledgeservice.Options) {
		catalog := options.Catalog
		options.Catalog = func(ctx context.Context) ([]store.KnowledgeStored, error) {
			entries, err := catalog(ctx)
			if err == nil {
				select {
				case passes <- entries:
				case <-ctx.Done():
					return nil, ctx.Err()
				}
			}
			return entries, err
		}
	}})
	nextPass := func() []store.KnowledgeStored {
		t.Helper()
		select {
		case entries := <-passes:
			return entries
		case <-time.After(10 * time.Second):
			t.Fatal("no knowledge indexing pass")
			return nil
		}
	}
	nextPass() // Consume the pass requested by Node startup.

	const name = "import-knowledge"
	source := t.TempDir()
	for path, body := range map[string]string{
		"plugin.json":                       `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write"],"storage":{"collections":[{"scope":"node","prefix":"entry/","schema":{"type":"object","properties":{"text":{"type":"string"}}},"index":{"semantic":true}}]},"settings_schema":"dev.sailry.platform/settings.json"}}}`,
		"dev.sailry.platform/settings.json": `{"$schema":"` + plugins.SettingsSchema + `","type":"object","properties":{"mode":{"type":"string"}},"additionalProperties":false}`,
	} {
		filename := filepath.Join(source, path)
		if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filename, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	info, err := node.plugins.manager.Install(ctx, source, "", name)
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("fixture package: %v, %v", info.Issues, err)
	}
	// Seed an existing source without a registration, as on a Node where the package was disabled at startup.
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		installed, err := store.InstallPlugin(ctx, db, info, 0, false)
		if err != nil {
			return err
		}
		index := protocol.DefaultPluginStorageIndex()
		index.Fields[0], index.Source = "fixture", &protocol.PluginStorageSource{Key: "entry/1", Pointer: "/text"}
		_, err = store.WritePluginValue(ctx, db, plugins.PackageReference(installed), "entry/1", json.RawMessage(`{"text":"fixture"}`), 0, &index)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	entry := store.ConfigurationPlugin{Name: name, Digest: info.Summary.Digest, Enabled: true}
	request := func(entry store.ConfigurationPlugin, preview bool) protocol.Request {
		t.Helper()
		document, err := json.Marshal(store.ConfigurationDocument{Format: "sailry-configuration", Version: 1, Plugins: []store.ConfigurationPlugin{entry}})
		if err != nil {
			t.Fatal(err)
		}
		return protocol.NewRequest(node.ID(), protocol.Command{Kind: "import_configuration", Data: &protocol.CommandImportConfigurationPayload{Document: document, Preview: preview}})
	}
	mustImport := func(entry store.ConfigurationPlugin, preview bool) {
		t.Helper()
		result := dispatchFixture(t, node, node.ID(), request(entry, preview))
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
	}
	eventCounts := func() (counts [2]int) {
		t.Helper()
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			return db.QueryRowContext(ctx, `SELECT COUNT(*) FILTER (WHERE json_extract(body,'$.kind')='plugin_changed'), COUNT(*) FILTER (WHERE json_extract(body,'$.kind')='knowledge_collections_changed') FROM events`).Scan(&counts[0], &counts[1])
		}); err != nil {
			t.Fatal(err)
		}
		return counts
	}
	before := eventCounts()
	mustImport(entry, true)
	exec := func(statement string) {
		t.Helper()
		if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
			_, err := db.ExecContext(ctx, statement)
			return err
		}); err != nil {
			t.Fatal(err)
		}
	}
	exec(`CREATE TRIGGER refuse_registration BEFORE INSERT ON knowledge_collections BEGIN SELECT RAISE(ABORT,'registration failed'); END`)
	if result := dispatchFixture(t, node, node.ID(), request(entry, false)); result.Fault == nil {
		t.Fatal("import committed despite a failed registration")
	}
	exec(`DROP TRIGGER refuse_registration`)
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		installed, err := store.ReadPlugin(ctx, db, name)
		if err != nil {
			return err
		}
		if installed == nil || installed.Summary.Enabled {
			t.Error("preview or failed import committed the enabled choice")
		}
		catalog, err := store.ReadKnowledgeCatalog(ctx, db)
		for _, stored := range catalog {
			if stored.Capture.Package != nil && stored.Capture.Package.Name == name {
				t.Error("preview or failed import committed a collection")
			}
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	mustImport(store.ConfigurationPlugin{Name: "not-installed", Enabled: true}, false)
	if got := eventCounts(); got != before {
		t.Fatalf("preview, failed or skipped import recorded package or knowledge events: %v -> %v", before, got)
	}
	select {
	case <-passes:
		t.Fatal("preview, failed or skipped import refreshed indexing")
	case <-time.After(time.Second):
	}

	capture := func() store.KnowledgeStored {
		t.Helper()
		for _, stored := range nextPass() {
			if stored.Automatic && stored.Capture.Package != nil && stored.Capture.Package.Name == name {
				return stored
			}
		}
		t.Fatal("indexing pass missed the imported package's collection")
		return store.KnowledgeStored{}
	}
	mustImport(entry, false)
	if got := eventCounts(); got != [2]int{before[0] + 1, before[1] + 1} {
		t.Fatalf("import did not commit both package and knowledge events: %v -> %v", before, got)
	}
	first := capture()
	entry.Values = map[string]json.RawMessage{"mode": json.RawMessage(`"updated"`)}
	pluginRequest := request(entry, false)
	scope := protocol.DefaultPluginContext()
	scope.Package = *first.Capture.Package
	pluginRequest.Plugin = &scope
	if result := dispatchFixture(t, node, node.ID(), pluginRequest); result.Fault == nil || result.Fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatalf("plugin-scoped import: %v", result.Fault)
	}
	mustImport(entry, false)
	updated := capture()
	if first.Collection.ID != updated.Collection.ID || updated.Collection.Progress.Generation <= first.Collection.Progress.Generation || updated.Capture.Package.SettingsRevision <= first.Capture.Package.SettingsRevision {
		t.Fatalf("settings import did not advance the existing collection: %+v -> %+v", first, updated)
	}
	// Disabling the package registers nothing but still changes what can be indexed.
	entry.Enabled, entry.Values = false, nil
	mustImport(entry, false)
	nextPass()
}
