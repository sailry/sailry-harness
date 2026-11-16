package node

import (
	"database/sql"
	"testing"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestKnowledgeCommandsRetainRemotePrincipal(t *testing.T) {
	node := nodeFixture(t)
	ctx := t.Context()
	callers := []protocol.NodeID{node.ID(), {93}, {94}}
	users := make([]protocol.User, len(callers))
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		for index, caller := range callers {
			if index != 0 {
				if err := store.PairLinkPeer(ctx, db, caller, link.DirectionController); err != nil {
					return err
				}
			}
			var err error
			users[index], err = store.CallerUser(ctx, db, node.ID(), caller)
			if err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	call := func(caller protocol.NodeID, command protocol.Command) link.Response {
		t.Helper()
		return dispatchFixture(t, node, caller, protocol.NewRequest(node.ID(), command))
	}
	must := func(caller protocol.NodeID, command protocol.Command) protocol.Output {
		t.Helper()
		result := call(caller, command)
		if result.Fault != nil {
			t.Fatalf("%s: %v", command.Kind, result.Fault)
		}
		return result.Output
	}
	collection := func(output protocol.Output) protocol.KnowledgeCollection {
		t.Helper()
		value, valid := dispatchPayload[protocol.KnowledgeCollection](output.Data)
		if !valid {
			t.Fatalf("collection output: %+v", output)
		}
		return value
	}
	listed := func(caller protocol.NodeID) []protocol.KnowledgeCollection {
		t.Helper()
		output := must(caller, protocol.Command{Kind: "list_knowledge_collections"})
		values, valid := dispatchPayload[[]protocol.KnowledgeCollection](output.Data)
		if !valid {
			t.Fatalf("collection list: %+v", output)
		}
		return values
	}
	owned := make([]protocol.KnowledgeCollection, len(callers))
	for index, caller := range callers {
		definition := protocol.KnowledgeDefinition{
			Name: "Private notes", Globs: []string{}, Tags: []string{}, SharedWith: []string{},
			Source: protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: t.TempDir()}},
		}
		owned[index] = collection(must(caller, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}}))
	}
	if err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		catalog, err := store.ReadKnowledgeCatalog(ctx, db)
		if err != nil {
			return err
		}
		for index, expected := range owned {
			for _, stored := range catalog {
				if stored.Collection.ID == expected.ID && (stored.Capture.User == nil || *stored.Capture.User != users[index].ID) {
					t.Errorf("caller %s created knowledge under another user", callers[index])
				}
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	for index, caller := range callers {
		entries := listed(caller)
		if len(entries) != 1 || entries[0].ID != owned[index].ID {
			t.Fatalf("caller %s sees another user's collections: %+v", caller, entries)
		}
		definition := owned[index].Definition
		definition.Name = "Updated private notes"
		owned[index] = collection(must(caller, protocol.Command{Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{
			ID: owned[index].ID, ExpectedRevision: owned[index].Revision, Definition: definition,
		}}))
		foreign := owned[(index+1)%len(owned)]
		for _, command := range []protocol.Command{
			{Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{ID: foreign.ID, ExpectedRevision: foreign.Revision, Definition: definition}},
			{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{ID: foreign.ID, ExpectedRevision: foreign.Revision, Paused: true}},
			{Kind: "rebuild_knowledge_collection", Data: &protocol.CommandRebuildKnowledgeCollectionPayload{ID: foreign.ID, ExpectedRevision: foreign.Revision}},
			{Kind: "delete_knowledge_collection", Data: &protocol.CommandDeleteKnowledgeCollectionPayload{ID: foreign.ID, ExpectedRevision: foreign.Revision}},
		} {
			if result := call(caller, command); result.Fault == nil {
				t.Fatalf("%s accepted another user's collection", command.Kind)
			}
		}
	}
	for index, caller := range callers {
		entries := listed(caller)
		if len(entries) != 1 || entries[0].ID != owned[index].ID || entries[0].Definition.Name != "Updated private notes" || entries[0].Revision != owned[index].Revision {
			t.Fatalf("foreign mutation changed caller %s's collection: %+v", caller, entries)
		}
		must(caller, protocol.Command{Kind: "delete_knowledge_collection", Data: &protocol.CommandDeleteKnowledgeCollectionPayload{ID: owned[index].ID, ExpectedRevision: owned[index].Revision}})
		if entries := listed(caller); len(entries) != 0 {
			t.Fatalf("caller %s's collection was not removed: %+v", caller, entries)
		}
	}
}
