package node

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestKnowledgeRecoveryRetainsForeignPluginScopeAndCurrentVisibility(t *testing.T) {
	node := nodeFixture(t)
	nodeID := node.ID()
	ctx := t.Context()
	if err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := store.LocalUser(ctx, db)
		if err != nil {
			return err
		}
		reader, err := store.CreateUser(ctx, db, "Recovered reader")
		if err != nil {
			return err
		}
		ownerCtx := store.WithUser(ctx, owner)
		// The scope was checked by a different execution Node; none of its
		// provenance IDs or package installations exist in this store.
		turn, session, worktree := protocol.NewTurnID(), protocol.NewSessionID(), protocol.NewWorktreeID()
		scope := protocol.DefaultPluginContext()
		scope.Package = protocol.PluginReference{Name: "remote-notes", Digest: "captured"}
		scope.Turn, scope.Session, scope.Worktree = &turn, &session, &worktree
		definition := protocol.DefaultKnowledgeDefinition()
		definition.Name, definition.Visibility = "Recovered notes", protocol.KnowledgeVisibilityShared
		definition.Source = protocol.KnowledgeSource{Kind: "storage", Data: &protocol.KnowledgeSourceStoragePayload{Scope: protocol.PluginStorageScopeNode}}
		request := protocol.NewRequest(nodeID, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}})
		request.Plugin = &scope
		body, err := json.Marshal(request)
		if err != nil {
			return err
		}
		info := protocol.PluginInfo{}
		info.Summary.Name, info.Summary.Digest = scope.Package.Name, scope.Package.Digest
		info.Extension = &protocol.PluginExtension{Actions: []protocol.PluginAction{protocol.PluginActionReadStorage, protocol.PluginActionReadKnowledge, protocol.PluginActionWriteKnowledge}}
		admitted := store.WithKnowledgePackage(ownerCtx, info)
		created, _, err := store.ExecuteKnowledge(admitted, db, request)
		if err != nil {
			return err
		}
		collection := created.Data.(protocol.KnowledgeCollection)
		if _, err := admission.Admit(admitted, db, nodeID, request, body); err != nil {
			return err
		}
		if err := admission.Complete(admitted, db, nodeID, request.ID, response(created, nil)); err != nil {
			return err
		}
		var saved []byte
		if err := db.QueryRowContext(ctx, "SELECT scope FROM requests WHERE caller=?1 AND id=?2", nodeID[:], request.ID.String()).Scan(&saved); err != nil {
			return err
		}
		var capture admission.Scope
		if err := json.Unmarshal(saved, &capture); err != nil || !capture.ExternalPlugin || len(capture.Resources) != 0 {
			return fmt.Errorf("foreign scope was treated as local: %s: %v", saved, err)
		}
		outcome, err := admission.Inspect(ownerCtx, db, nodeID, request.ID, admission.Digest(body))
		if err != nil {
			return err
		}
		recovered, err := node.inspectOutcomeAccess(ownerCtx, db, nodeID, request.ID, outcome)
		value, valid := dispatchPayload[protocol.Result[protocol.Output, protocol.Fault]](recovered.Data)
		if err != nil || !valid || value.Ok == nil {
			return fmt.Errorf("foreign plugin receipt did not recover: %+v: %v", recovered, err)
		}
		recovered, err = node.inspectOutcomeAccess(store.WithUser(ctx, reader), db, nodeID, request.ID, outcome)
		value, valid = dispatchPayload[protocol.Result[protocol.Output, protocol.Fault]](recovered.Data)
		if err != nil || !valid || value.Err == nil {
			return fmt.Errorf("another actor recovered the receipt: %+v: %v", recovered, err)
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM knowledge_collections WHERE id=?1", collection.ID.String()); err != nil {
			return err
		}
		recovered, err = node.inspectOutcomeAccess(ownerCtx, db, nodeID, request.ID, outcome)
		value, valid = dispatchPayload[protocol.Result[protocol.Output, protocol.Fault]](recovered.Data)
		if err != nil || !valid || value.Err == nil {
			return fmt.Errorf("removed collection remained in recovered output: %+v: %v", recovered, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
