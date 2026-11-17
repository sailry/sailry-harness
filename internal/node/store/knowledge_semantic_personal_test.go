package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPersonalKVAcceptedGroupsRefreshSemanticCollections(t *testing.T) {
	source := newPersonalKVFixture(t, protocol.NodeID{})
	ctx := source.ctx
	if err := source.store.Node.Write(ctx, func(db *sql.Conn) error {
		info := pluginTestInfo("personal-indexed", 'f')
		extension := protocol.DefaultPluginExtension()
		extension.ApiVersion = "v1"
		extension.Actions = []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage}
		prefix := "entry/"
		extension.Storage = &protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{
			Scope: protocol.PluginStorageScopeNode, Prefix: &prefix, Schema: json.RawMessage(`{"type":"object"}`),
			Index: &protocol.PluginStorageDeclarationIndex{Semantic: true},
		}}}
		info.Extension = &extension
		installed, err := InstallPlugin(ctx, db, info, 0, true)
		if err == nil {
			source.reference = plugins.PackageReference(installed)
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	executor := protocol.NodeID{91}
	group := func(sequence uint64, key string, base uint64, text string, index *protocol.PluginStorageIndex) protocol.PersonalKVGroup {
		t.Helper()
		value := protocol.PersonalKVGroup{Controller: source.node, Executor: executor, Request: protocol.NewRequestID(), Package: source.reference,
			Sequence: sequence, Writes: []protocol.PersonalKVWrite{{Key: key, BaseRevision: base, Present: true, Value: json.RawMessage(text), Index: index}}}
		var err error
		value.Digest, err = configurationDigest(value)
		if err != nil {
			t.Fatal(err)
		}
		return value
	}
	apply := func(value protocol.PersonalKVGroup, applied, refreshed bool) {
		t.Helper()
		if err := source.store.Node.Write(ctx, func(db *sql.Conn) error {
			receipt, changes, err := ApplyPersonalKVGroup(ctx, db, source.node, executor, value)
			if err != nil {
				return err
			}
			if receipt.Applied != applied || slices.ContainsFunc(changes, func(change protocol.Event) bool {
				return change.Kind == "knowledge_collections_changed"
			}) != refreshed {
				return fmt.Errorf("semantic change did not match receipt: %+v %+v", receipt, changes)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	read := func() KnowledgeStored {
		t.Helper()
		var selected KnowledgeStored
		if err := source.store.Node.Read(ctx, func(db *sql.Conn) error {
			entries, err := ReadKnowledgeCatalog(ctx, db)
			if err != nil {
				return err
			}
			for _, entry := range entries {
				if entry.Automatic && entry.Capture.Package.Name == source.reference.Name {
					selected = entry
					return nil
				}
			}
			return fmt.Errorf("accepted semantic source did not register its collection")
		}); err != nil {
			t.Fatal(err)
		}
		return selected
	}
	apply(group(1, "entry/raw", 0, `{"text":"first source"}`, nil), true, false)
	index := &protocol.PluginStorageIndex{Fields: [2]string{"entry", ""}, Source: &protocol.PluginStorageSource{Key: "entry/raw", Pointer: "/text"}}
	indexed := group(2, "entry/record", 0, `{"title":"record"}`, index)
	apply(indexed, true, true)
	registered := read()
	if registered.Collection.Progress.Status != protocol.KnowledgeStatusPending || registered.Collection.Progress.Generation != 1 {
		t.Fatalf("first remote indexed source has no pending registration: %+v", registered.Collection.Progress)
	}
	apply(indexed, true, false)
	if read().Collection.Progress.Generation != registered.Collection.Progress.Generation {
		t.Fatal("a repeated receipt advanced semantic generation")
	}
	if err := source.store.Node.Write(ctx, func(db *sql.Conn) error {
		progress := protocol.DefaultKnowledgeProgress()
		progress.Status, progress.Generation = protocol.KnowledgeStatusReady, registered.Collection.Progress.Generation
		_, err := RecordKnowledgeProgress(ctx, db, registered.Collection.ID, progress.Generation, progress)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	apply(group(3, "entry/raw", 1, `{"text":"second source"}`, nil), true, true)
	changed := read()
	if changed.Collection.ID != registered.Collection.ID || changed.Collection.Progress.Generation != registered.Collection.Progress.Generation+1 {
		t.Fatal("an accepted source edit left the canonical semantic index unchanged")
	}
	apply(group(4, "entry/raw", 0, `{"text":"conflicting source"}`, nil), false, false)
	if read().Collection.Progress.Generation != changed.Collection.Progress.Generation {
		t.Fatal("a rejected source edit advanced semantic generation")
	}
	for offset, removed := range []bool{false, true} {
		if err := source.store.Node.Write(ctx, func(db *sql.Conn) error {
			info, err := ReadPlugin(ctx, db, source.reference.Name)
			if err != nil || info == nil {
				return fmt.Errorf("fixture package: %v", err)
			}
			if removed {
				_, err = RemovePlugin(ctx, db, info.Summary.Name, info.Summary.Revision)
			} else {
				_, err = SetPluginEnabled(ctx, db, info.Summary.Name, info.Summary.Revision, false)
			}
			return err
		}); err != nil {
			t.Fatal(err)
		}
		apply(group(uint64(5+offset), "entry/raw", uint64(2+offset), `{"text":"retained after removal"}`, nil), true, false)
		if read().Collection.Progress.Generation != changed.Collection.Progress.Generation {
			t.Fatal("disabled or removed package restarted semantic indexing")
		}
		if err := source.store.Node.Read(ctx, func(db *sql.Conn) error {
			record, err := personalKVRecord(ctx, db, changed.Capture.Space, source.reference.Name, "entry/raw")
			if err != nil || !record.Present || record.Revision != uint64(3+offset) {
				return fmt.Errorf("disabled package lost accepted personal data: %+v: %v", record, err)
			}
			info, err := ReadPlugin(ctx, db, source.reference.Name)
			if err != nil || !removed && (info == nil || info.Summary.Enabled) || removed && info != nil {
				return fmt.Errorf("personal receipt changed installed package state: %+v: %v", info, err)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
}
