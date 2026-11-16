package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginSearchAcrossTransactions(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	var reference protocol.PluginReference
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("search-commits", 'a')
		prefix := ""
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage}
		extension.Storage = &protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{
			Scope: protocol.PluginStorageScopeNode, Prefix: &prefix, Schema: json.RawMessage(`{"type":"object"}`), Index: &protocol.PluginStorageDeclarationIndex{Semantic: true},
		}}}
		info.Extension = &extension
		info, err := InstallPlugin(ctx, db, info, 0, true)
		reference = plugins.PackageReference(info)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	write := func(key string, revision uint64, index *protocol.PluginStorageIndex) uint64 {
		t.Helper()
		var next uint64
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			entry, err := WritePluginValue(ctx, db, reference, key, json.RawMessage(`{"text":"search content"}`), revision, index)
			next = entry.Revision
			if err != nil {
				return err
			}
			_, _, err = ensureSemantic(ctx, db, reference)
			return err
		}); err != nil {
			t.Fatalf("write %s: %v", key, err)
		}
		return next
	}
	write("source", 0, nil)
	index := &protocol.PluginStorageIndex{Fields: [2]string{"Review changes", "资料检索"}, Tags: []string{"review"}, Order: 3}
	revision := write("record", 0, index)
	search := func(term string, expected int) {
		t.Helper()
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			query := protocol.DefaultPluginStorageSearch()
			query.Query, query.Weights, query.All, query.Limit = &term, [2]uint16{3, 1}, []string{"review"}, 10
			page, err := SearchPluginValues(ctx, db, reference, query)
			if err != nil {
				return err
			}
			if len(page.Entries) != expected {
				return fmt.Errorf("search %q returned %d entries, want %d", term, len(page.Entries), expected)
			}
			return nil
		}); err != nil {
			t.Fatalf("committed search: %v", err)
		}
	}
	search("review", 1)
	search("资料", 1)
	write("source", 1, nil)
	index.Fields[0] = "Updated content"
	revision = write("record", revision, index)
	search("review", 0)
	search("updated", 1)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		_, err := RemovePluginValue(ctx, db, reference, "record", revision)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	search("updated", 0)
}
