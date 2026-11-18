package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationCacheAndPersonalSnapshotAvoidDuplicateSemanticIndex(t *testing.T) {
	ctx := t.Context()
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
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
	var manifests []protocol.ConfigurationManifest
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		if err := store.PairLinkPeer(ctx, db, source, link.DirectionController); err != nil {
			return err
		}
		var err error
		receiver, err = store.CallerUser(ctx, db, node.ID(), source)
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "UPDATE users SET controller=?2 WHERE id=?1", receiver.ID.String(), source[:]); err != nil {
			return err
		}
		receiver.Controller = &source
		// The source namespace supplies real export records and immutable
		// package content. The executor receives no authoritative installation.
		info, err = store.InstallPlugin(ctx, db, info, 0, true)
		if err != nil {
			return err
		}
		if err := store.CacheConfigurationPackage(ctx, db, info, protocol.PluginMcpConfiguration{Servers: map[string]protocol.PluginMcpServer{}}); err != nil {
			return err
		}
		for _, category := range []string{"plugins", "plugin_settings"} {
			manifest, err := store.ExportConfigurationManifest(ctx, db, source, node.ID(), category, true, nil, nil)
			if err != nil {
				return err
			}
			selected := manifest.Records[:0]
			for _, record := range manifest.Records {
				if record.ID == info.Summary.Name || strings.HasPrefix(record.ID, info.Summary.Name+":") {
					selected = append(selected, record)
				}
			}
			manifest.Records = selected
			if len(selected) != 1 {
				return fmt.Errorf("missing %s export", category)
			}
			manifests = append(manifests, manifest)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	for _, manifest := range manifests {
		result := dispatchFixture(t, node, source, protocol.NewRequest(node.ID(), protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply", Manifest: &manifest}}))
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
	}
	body := json.RawMessage(`{"text":"followedsemantic fixture content"}`)
	snapshot := protocol.PersonalKVSnapshot{Generation: 1, Records: []protocol.PersonalKVRecord{
		{Plugin: info.Summary.Name, Key: "entry/raw", Revision: 1, Present: true, Value: body},
		{Plugin: info.Summary.Name, Key: "entry/record", Revision: 1, Present: true, Value: body, Index: &protocol.PluginStorageIndex{
			Fields: [2]string{"followedsemantic fixture content", ""}, Source: &protocol.PluginStorageSource{Key: "entry/raw", Pointer: "/text"},
		}},
	}}
	result := dispatchFixture(t, node, source, protocol.NewRequest(node.ID(), protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "kv_snapshot", KVSnapshot: &snapshot}}))
	if result.Fault != nil {
		t.Fatal(result.Fault)
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		owner := store.WithUser(ctx, receiver)
		entry, err := store.ReadPluginValue(owner, db, plugins.PackageReference(info), "entry/raw")
		if err != nil || !entry.Present || string(entry.Value) != string(body) {
			return fmt.Errorf("personal snapshot value unavailable: %v", err)
		}
		catalog, err := store.ReadKnowledgeCatalog(ctx, db)
		if err != nil {
			return err
		}
		for _, stored := range catalog {
			if stored.Capture.Package != nil && stored.Capture.Package.Name == info.Summary.Name && stored.Capture.User != nil && *stored.Capture.User == receiver.ID {
				return fmt.Errorf("executor created a duplicate personal semantic index: %s", stored.Collection.ID)
			}
		}
		var formal int
		if err := db.QueryRowContext(ctx, "SELECT count(*) FROM plugin_user_installations WHERE user=?1 AND name=?2", receiver.ID.String(), info.Summary.Name).Scan(&formal); err != nil {
			return err
		}
		if formal != 0 {
			return fmt.Errorf("configuration cache materialized %d authoritative installations", formal)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
