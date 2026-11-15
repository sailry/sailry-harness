package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationFollowRegistersAndIndexesSemanticStorage(t *testing.T) {
	ctx := t.Context()
	initialCatalog := make(chan struct{})
	var initial sync.Once
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment, knowledge: func(options *knowledgeservice.Options) {
		catalog := options.Catalog
		options.Catalog = func(ctx context.Context) ([]store.KnowledgeStored, error) {
			entries, err := catalog(ctx)
			initial.Do(func() { close(initialCatalog) })
			return entries, err
		}
		model := localmodels.DefaultEmbedding
		options.Choice = func() protocol.ModelsLocalChoice { return protocol.ModelsLocalChoice{Enabled: true, Model: &model} }
		options.Dimension = func(id string) (int, string, bool) { return 2, "follow-fixture", id == model }
		options.Installed = func() (map[string]bool, error) { return map[string]bool{model: true}, nil }
		options.Offsets = func(context.Context, string, protocol.ModelsLocalChoice, string) ([][2]int, error) { return nil, nil }
		options.Encode = func(_ context.Context, _ string, _ protocol.ModelsLocalChoice, texts []string, _ bool) ([][]float32, error) {
			vectors := make([][]float32, len(texts))
			for index := range vectors {
				vectors[index] = []float32{1, 0}
			}
			return vectors, nil
		}
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	// The startup pass has already captured its empty catalog. Only a later
	// committed follow change can make the worker discover the new collection.
	select {
	case <-initialCatalog:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	directory := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"follow-knowledge","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["storage.read","storage.write"],"storage":{"collections":[{"scope":"node","prefix":"entry/","schema":{"type":"object","properties":{"text":{"type":"string"}}},"index":{"semantic":true}}]}}}}`
	if err := os.WriteFile(filepath.Join(directory, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := node.plugins.manager.Install(ctx, directory, "", "follow-knowledge")
	if err != nil || len(info.Issues) != 0 {
		t.Fatalf("package %v: %#v", err, info.Issues)
	}
	source := protocol.NodeID{97}
	var receiver protocol.User
	var followed protocol.ConfigurationManifest
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, source, link.DirectionController); err != nil {
			return err
		}
		var err error
		receiver, err = store.CallerUser(ctx, db, node.ID(), source)
		if err != nil {
			return err
		}
		// The local namespace supplies real export records; the receiving
		// user has retained values but has not installed the followed package.
		info, err = store.InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		if err := store.CacheConfigurationPackage(ctx, db, info, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{}}); err != nil {
			return err
		}
		owner := store.WithCapturedUser(store.WithUser(ctx, receiver), receiver.ID)
		body := json.RawMessage(`{"text":"followedsemantic fixture content"}`)
		reference := plugins.PackageReference(info)
		if _, err := store.WritePluginValue(owner, db, reference, "entry/raw", body, 0, nil); err != nil {
			return err
		}
		if _, err := store.WritePluginValue(owner, db, reference, "entry/record", body, 0, &protocol.PluginStorageIndex{
			Fields: [2]string{"followedsemantic fixture content", ""}, Source: &protocol.PluginStorageSource{Key: "entry/raw", Pointer: "/text"},
		}); err != nil {
			return err
		}
		followed, err = store.ExportConfigurationManifest(ctx, db, source, node.ID(), "plugins", false)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	records := followed.Records
	followed.Records = nil
	for _, record := range records {
		if record.ID == info.Summary.Name {
			followed.Records = []protocol.ConfigurationFollowRecord{record}
			break
		}
	}
	if len(followed.Records) != 1 {
		t.Fatal("semantic package is missing from its configuration export")
	}
	result := dispatchFixture(t, node, source, protocol.NewRequest(node.ID(), protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply", Manifest: &followed}}))
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	var registered store.KnowledgeStored
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		catalog, err := store.ReadKnowledgeCatalog(ctx, db)
		if err != nil {
			return err
		}
		for _, stored := range catalog {
			if stored.Capture.Package != nil && stored.Capture.Package.Name == info.Summary.Name && stored.Capture.Space == "user:"+receiver.ID.String() {
				registered = stored
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if registered.Capture.User == nil || *registered.Capture.User != receiver.ID {
		t.Fatal("follow apply did not register the receiving user's semantic collection")
	}
	for deadline := time.Now().Add(30 * time.Second); ; time.Sleep(20 * time.Millisecond) {
		if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			catalog, err := store.ReadKnowledgeCatalog(ctx, db)
			if err != nil {
				return err
			}
			for _, stored := range catalog {
				if stored.Collection.ID == registered.Collection.ID {
					registered = stored
				}
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		if registered.Collection.Progress.Status == protocol.KnowledgeStatusReady && registered.Collection.Progress.Sources == 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("follow apply did not wake the semantic worker: %+v", registered.Collection.Progress)
		}
	}
	query := protocol.DefaultKnowledgeQuery()
	query.Query, query.Mode, query.Limit = "followedsemantic", protocol.KnowledgeModeKeyword, 5
	result = dispatchFixture(t, node, source, protocol.NewRequest(node.ID(), protocol.Command{Kind: "query_knowledge", Data: &query}))
	page, valid := dispatchPayload[protocol.KnowledgePage](result.Output.Data)
	if result.Fault != nil || !valid || len(page.Hits) != 1 || page.Hits[0].Text != "followedsemantic fixture content" {
		t.Fatalf("followed storage is not indexed for its receiving user: %+v", result)
	}
}
