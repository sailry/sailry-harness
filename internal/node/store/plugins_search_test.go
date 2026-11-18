package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
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

func TestPairedUserSemanticStorageSurvivesOrphanSweep(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	var reference protocol.PluginReference
	var collection protocol.KnowledgeID
	controller := protocol.NodeID{44}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if err := PairPeerIdentity(ctx, db, node, controller, link.DirectionController, true); err != nil {
			return err
		}
		actor, err := CallerUser(ctx, db, node, controller)
		if err != nil {
			return err
		}
		owner := WithUser(ctx, actor)
		prefix := "notes/"
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage}
		extension.Storage = &protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{
			Scope: protocol.PluginStorageScopeNode, Prefix: &prefix, Schema: json.RawMessage(`{"type":"object"}`), Index: &protocol.PluginStorageDeclarationIndex{Semantic: true},
		}}}
		info := pluginTestInfo("paired-search", 'a')
		info.Extension = &extension
		installed, err := InstallPlugin(owner, db, info, 0, true)
		if err != nil {
			return err
		}
		reference = plugins.PackageReference(installed)
		value := json.RawMessage(`{"text":"fixture knowledge"}`)
		if _, err := WritePluginValue(owner, db, reference, "notes/source", value, 0, nil); err != nil {
			return err
		}
		index := &protocol.PluginStorageIndex{
			Fields: [2]string{"Fixture notes", ""},
			Source: &protocol.PluginStorageSource{Key: "notes/source", Pointer: "/text"},
		}
		if _, err := WritePluginValue(owner, db, reference, "notes/index", value, 0, index); err != nil {
			return err
		}
		registered, _, err := ensureSemantic(owner, db, reference)
		if err != nil {
			return err
		}
		if len(registered) != 1 {
			return fmt.Errorf("registered %d semantic collections, want one", len(registered))
		}
		collection = registered[0].Collection.ID
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		remaining, changed, err := RemoveOrphanedKnowledge(ctx, db)
		if err != nil {
			return err
		}
		if changed || len(remaining) != 1 || remaining[0].Collection.ID != collection {
			return fmt.Errorf("orphan sweep changed the paired user's semantic catalog: %d collections, changed=%t", len(remaining), changed)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		actor, err := CallerUser(ctx, db, node, controller)
		if err != nil {
			return err
		}
		term := "fixture"
		query := protocol.DefaultPluginStorageSearch()
		query.Query, query.Mode, query.Weights, query.Limit = &term, protocol.KnowledgeModeSemantic, [2]uint16{1, 1}, 10
		request := protocol.NewRequest(node, protocol.Command{Kind: "search_plugin_values", Data: &query})
		request.Plugin = &protocol.PluginContext{Package: reference}
		plan, err := PrepareSemanticSearch(WithUser(ctx, actor), db, request)
		if err != nil {
			return err
		}
		if len(plan.Collections) != 1 || plan.Collections[0].Collection.ID != collection || !plan.Eligible["notes/index"] {
			return fmt.Errorf("semantic search lost the paired user's local index")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
