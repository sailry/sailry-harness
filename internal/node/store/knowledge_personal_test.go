package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPersonalKnowledgeUsesLocalViewRevisionAndDropsChangedSources(t *testing.T) {
	fixture := newPersonalKVFixture(t, protocol.NodeID{3})
	ctx := fixture.ctx
	if err := fixture.store.Node.Write(ctx, func(db *sql.Conn) error {
		prefix := "entry/"
		info, err := ResolvePluginInfo(ctx, db, fixture.reference)
		if err != nil {
			return err
		}
		info.Extension.Storage = &protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{Scope: protocol.PluginStorageScopeNode, Prefix: &prefix, Schema: json.RawMessage(`{"type":"object"}`), Index: &protocol.PluginStorageDeclarationIndex{Semantic: true}}}}
		ctx = WithKnowledgePackage(ctx, info)
		user, err := CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		space := "controller:" + user.ID.String()
		if err := savePersonalKVRecord(ctx, db, space, protocol.PersonalKVRecord{Plugin: fixture.reference.Name, Key: "entry/raw", Present: true, Value: json.RawMessage(`{"text":"controller source"}`)}, 18); err != nil {
			return err
		}
		index := &protocol.PluginStorageIndex{Fields: [2]string{"semantic entry", ""}, Source: &protocol.PluginStorageSource{Key: "entry/raw", Pointer: "/text"}}
		if err := savePersonalKVRecord(ctx, db, space, protocol.PersonalKVRecord{Plugin: fixture.reference.Name, Key: "entry/record", Present: true, Value: json.RawMessage(`{"key":"record"}`), Index: index}, 42); err != nil {
			return err
		}
		declarations, err := semanticDeclarations(ctx, db, fixture.reference)
		if err != nil {
			return err
		}
		record, err := semanticRecord(ctx, db, fixture.reference, declarations, "entry/record")
		if err != nil || record == nil {
			return fmt.Errorf("semantic record missing: %v", err)
		}
		query := protocol.DefaultPluginStorageSearch()
		text := "unmatched text"
		query.Query, query.Mode = &text, protocol.KnowledgeModeSemantic
		request := protocol.NewRequest(fixture.node, protocol.Command{Kind: "search_plugin_values", Data: &query})
		request.Plugin = &protocol.PluginContext{Package: fixture.reference}
		candidates := protocol.KnowledgeCandidates{Available: true, Candidates: []protocol.KnowledgeCandidate{{Key: record.Key, Revision: record.Identity()}}}
		page, err := MaterializePersonalKnowledge(ctx, db, request, candidates)
		if err != nil || len(page.Entries) != 1 || page.Entries[0].Revision != 42 || page.Mode != protocol.KnowledgeModeSemantic {
			return fmt.Errorf("remote candidate lost local view: %+v: %v", page, err)
		}
		if err := savePersonalKVRecord(ctx, db, space, protocol.PersonalKVRecord{Plugin: fixture.reference.Name, Key: "entry/raw", Present: true, Value: json.RawMessage(`{"text":"pending local source"}`)}, 19); err != nil {
			return err
		}
		page, err = MaterializePersonalKnowledge(ctx, db, request, candidates)
		if err != nil || len(page.Entries) != 0 || !page.Indexing {
			return fmt.Errorf("changed overlay exposed stale remote source: %+v: %v", page, err)
		}
		collections, changed, err := ensureSemantic(ctx, db, plugins.PackageReference(info))
		if err != nil || changed || len(collections) != 0 {
			return fmt.Errorf("replica created a duplicate semantic index: %+v: %v", collections, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
