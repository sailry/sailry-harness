package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestKnowledgeVisibilityAndOwnerManagement(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		reader, err := CreateUser(ctx, db, "Knowledge reader")
		if err != nil {
			return err
		}
		ownerCtx, readerCtx := WithUser(ctx, owner), WithUser(ctx, reader)
		definition := protocol.DefaultKnowledgeDefinition()
		definition.Name = "Shared notes"
		definition.Visibility = protocol.KnowledgeVisibilityShared
		definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: t.TempDir()}}
		created, _, err := ExecuteKnowledge(ownerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}}))
		if err != nil {
			return err
		}
		collection := created.Data.(protocol.KnowledgeCollection)
		if collection.Node != node || collection.OwnerUser != owner.ID || ReadKnowledgeSource(collection.Definition.Source).Node == nil || *ReadKnowledgeSource(collection.Definition.Source).Node != node {
			return fmt.Errorf("collection lost storage identity: %+v", collection)
		}
		entries, err := ListKnowledgeCollections(readerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "list_knowledge_collections"}))
		if err != nil || len(entries) != 1 || entries[0].ID != collection.ID {
			return fmt.Errorf("shared collection read: %+v: %v", entries, err)
		}
		for _, command := range []protocol.Command{
			{Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: collection.Revision, Definition: definition}},
			{Kind: "pause_knowledge_collection", Data: &protocol.CommandPauseKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: collection.Revision, Paused: true}},
			{Kind: "rebuild_knowledge_collection", Data: &protocol.CommandRebuildKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: collection.Revision}},
			{Kind: "delete_knowledge_collection", Data: &protocol.CommandDeleteKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: collection.Revision}},
		} {
			if _, _, err := ExecuteKnowledge(readerCtx, db, protocol.NewRequest(node, command)); err == nil {
				return fmt.Errorf("reader managed another user's shared collection with %s", command.Kind)
			}
		}
		definition.Visibility = protocol.KnowledgeVisibilityPrivate
		if _, _, err := ExecuteKnowledge(ownerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "update_knowledge_collection", Data: &protocol.CommandUpdateKnowledgeCollectionPayload{ID: collection.ID, ExpectedRevision: collection.Revision, Definition: definition}})); err != nil {
			return err
		}
		entries, err = ListKnowledgeCollections(readerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "list_knowledge_collections"}))
		if err != nil || len(entries) != 0 {
			return fmt.Errorf("sharing revocation did not apply: %+v: %v", entries, err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestKnowledgeLegacyLocationAndWrongNodeSource(t *testing.T) {
	t.Parallel()
	_, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		reader, err := CreateUser(ctx, db, "Legacy reader")
		if err != nil {
			return err
		}
		collection := protocol.DefaultKnowledgeCollection()
		collection.ID, collection.Revision, collection.Progress.Generation = protocol.NewKnowledgeID(), 1, 1
		collection.Owner = protocol.KnowledgeOwner{Kind: "controller"}
		collection.Definition.Name = "Legacy notes"
		collection.Definition.Visibility = ""
		collection.Definition.Source = protocol.KnowledgeSource{Kind: "directory", Data: &protocol.KnowledgeSourceDirectoryPayload{Path: t.TempDir()}}
		body, err := json.Marshal(KnowledgeStored{Collection: collection, Capture: KnowledgeCapture{User: &owner.ID}})
		if err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO knowledge_collections(id,revision,generation,body) VALUES(?1,1,1,?2)", collection.ID.String(), body); err != nil {
			return err
		}
		entries, err := ListKnowledgeCollections(WithUser(ctx, owner), db, protocol.NewRequest(node, protocol.Command{Kind: "list_knowledge_collections"}))
		if err != nil || len(entries) != 1 || entries[0].Node != node || entries[0].OwnerUser != owner.ID || entries[0].Definition.Visibility != protocol.KnowledgeVisibilityPrivate {
			return fmt.Errorf("legacy projection: %+v: %v", entries, err)
		}
		entries, err = ListKnowledgeCollections(WithUser(ctx, reader), db, protocol.NewRequest(node, protocol.Command{Kind: "list_knowledge_collections"}))
		if err != nil || len(entries) != 0 {
			return fmt.Errorf("legacy collection became shared: %+v: %v", entries, err)
		}
		var unchanged []byte
		if err := db.QueryRowContext(ctx, "SELECT body FROM knowledge_collections WHERE id=?1", collection.ID.String()).Scan(&unchanged); err != nil {
			return err
		}
		if string(unchanged) != string(body) {
			return fmt.Errorf("reading a legacy collection rewrote its source")
		}
		foreign := protocol.NodeID{9}
		definition := collection.Definition
		definition.Source = protocol.KnowledgeSource{Kind: "file", Data: &protocol.KnowledgeSourceFilePayload{Node: &foreign, Path: filepath.Join(t.TempDir(), "missing")}}
		_, _, err = ExecuteKnowledge(WithUser(ctx, owner), db, protocol.NewRequest(node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}}))
		if !providerFixtureFault(err, protocol.ErrorCodeInvalidRequest) {
			return fmt.Errorf("foreign Node source touched local filesystem: %v", err)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestKnowledgeProjectScopeOverridesIndependentSharing(t *testing.T) {
	t.Parallel()
	profile, node, storage := schemaFixture(t)
	ctx := t.Context()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		owner, err := LocalUser(ctx, db)
		if err != nil {
			return err
		}
		reader, err := CreateUser(ctx, db, "Project knowledge reader")
		if err != nil {
			return err
		}
		ownerCtx, readerCtx := WithUser(ctx, owner), WithUser(ctx, reader)
		path := t.TempDir()
		output, _, err := ExecuteCommand(ownerCtx, db, node, node, profile, protocol.NewRequest(node, protocol.Command{Kind: "register_project", Data: &protocol.CommandRegisterProjectPayload{Name: "Knowledge project", Path: path, Visibility: protocol.ProjectVisibilityPrivate}}), PreparedPaths{Path: path})
		if err != nil {
			return err
		}
		project := output.Data.(protocol.Project)
		var worktreeText string
		if err := db.QueryRowContext(ctx, "SELECT id FROM worktrees WHERE project=?1 AND main=1", project.ID.String()).Scan(&worktreeText); err != nil {
			return err
		}
		worktree, err := protocol.ParseWorktreeID(worktreeText)
		if err != nil {
			return err
		}
		definition := protocol.DefaultKnowledgeDefinition()
		definition.Name, definition.Visibility = "Project notes", protocol.KnowledgeVisibilityShared
		definition.Source = protocol.KnowledgeSource{Kind: "worktree", Data: &protocol.KnowledgeSourceWorktreePayload{Worktree: worktree, Path: "."}}
		if _, _, err := ExecuteKnowledge(readerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}})); err == nil {
			return fmt.Errorf("reader selected another user's private project source")
		}
		created, _, err := ExecuteKnowledge(ownerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "create_knowledge_collection", Data: &protocol.CommandCreateKnowledgeCollectionPayload{Definition: definition}}))
		if err != nil {
			return err
		}
		if created.Data.(protocol.KnowledgeCollection).Definition.Visibility != protocol.KnowledgeVisibilityPrivate {
			return fmt.Errorf("independent sharing exposed a private project")
		}
		for _, visibility := range []protocol.ProjectVisibility{protocol.ProjectVisibilityPrivate, protocol.ProjectVisibilityShared, protocol.ProjectVisibilityPrivate} {
			if _, err := db.ExecContext(ctx, "UPDATE projects SET visibility=?2 WHERE id=?1", project.ID.String(), visibility); err != nil {
				return err
			}
			entries, err := ListKnowledgeCollections(readerCtx, db, protocol.NewRequest(node, protocol.Command{Kind: "list_knowledge_collections"}))
			if err != nil || (len(entries) == 1) != (visibility == protocol.ProjectVisibilityShared) {
				return fmt.Errorf("project visibility %s: %+v: %v", visibility, entries, err)
			}
			if len(entries) != 0 && entries[0].Definition.Visibility != protocol.KnowledgeVisibilityShared {
				return fmt.Errorf("project collection reports stale visibility")
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestKnowledgeTrustedPackageDoesNotResolveForeignTurn(t *testing.T) {
	t.Parallel()
	_, _, storage := schemaFixture(t)
	ctx := t.Context()
	info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "remote-reader", Digest: "captured", SettingsRevision: 7}, Extension: &protocol.PluginExtension{ApiVersion: "v1", Actions: []protocol.PluginAction{protocol.PluginActionReadKnowledge}}}
	turn, session, worktree := protocol.NewTurnID(), protocol.NewSessionID(), protocol.NewWorktreeID()
	origin := protocol.PluginContext{Package: protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision}, Turn: &turn, Session: &session, Worktree: &worktree}
	trusted, err := WithKnowledgeScope(ctx, &protocol.ResourceScope{Plugin: &info, Origin: &origin})
	if err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		resolved, err := knowledgePackage(trusted, db, origin)
		if err != nil || resolved.Summary.Name != info.Summary.Name {
			return fmt.Errorf("trusted foreign capture required a local turn or installation: %v", err)
		}
		request := protocol.Request{Plugin: &origin, Command: protocol.Command{Kind: "list_knowledge_collections"}}
		if err := CheckKnowledgeScope(trusted, request); err != nil {
			return err
		}
		request.Command = protocol.Command{Kind: "delete_knowledge_collection"}
		if err := CheckKnowledgeScope(trusted, request); err == nil {
			return fmt.Errorf("read declaration authorized a mutation")
		}
		origin.Package.SettingsRevision++
		if _, err := knowledgePackage(trusted, db, origin); err == nil {
			return fmt.Errorf("trusted capture accepted another package revision")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
