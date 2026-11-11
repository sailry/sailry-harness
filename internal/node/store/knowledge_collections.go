package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func knowledgeModel(ctx context.Context, db *sql.Conn, id *string) error {
	if id == nil {
		return nil
	}
	definition, err := LocalModelDefinition(ctx, db, *id)
	if err != nil {
		return err
	}
	if localmodels.Kind(definition.Format) != protocol.ModelsKindEmbedding {
		return fault(protocol.ErrorCodeNotFound, "local model is not registered for its kind")
	}
	return nil
}

func loadKnowledge(ctx context.Context, db *sql.Conn) ([]KnowledgeStored, error) {
	rows, err := db.QueryContext(ctx, "SELECT id,revision,generation,body FROM knowledge_collections ORDER BY id")
	if err != nil {
		return nil, err
	}
	type record struct {
		id                   string
		revision, generation int64
		body                 []byte
	}
	var records []record
	for rows.Next() {
		var next record
		if err := rows.Scan(&next.id, &next.revision, &next.generation, &next.body); err != nil {
			rows.Close()
			return nil, err
		}
		records = append(records, next)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	entries := make([]KnowledgeStored, 0, len(records))
	for _, record := range records {
		var stored KnowledgeStored
		if err := json.Unmarshal(record.body, &stored); err != nil {
			return nil, err
		}
		if err := checkKnowledge(stored); err != nil {
			return nil, err
		}
		if err := knowledgeModel(ctx, db, stored.Collection.Definition.Model); err != nil {
			return nil, err
		}
		if record.id != stored.Collection.ID.String() || record.revision != int64(stored.Collection.Revision) || record.generation != int64(stored.Collection.Progress.Generation) {
			return nil, errors.New("knowledge metadata does not match its stored definition")
		}
		entries = append(entries, stored)
	}
	if knowledgeManual(entries) > MaxKnowledgeCollections {
		return nil, errors.New("knowledge collection capacity exceeded")
	}
	return entries, nil
}

// knowledgeManual counts the collections bounded by the capacity. Automatic
// registrations follow package manifests and are not counted.
func knowledgeManual(entries []KnowledgeStored) int {
	count := 0
	for _, stored := range entries {
		if !stored.Automatic {
			count++
		}
	}
	return count
}

func persistKnowledge(ctx context.Context, db *sql.Conn, stored KnowledgeStored) error {
	if err := checkKnowledge(stored); err != nil {
		return err
	}
	if err := knowledgeModel(ctx, db, stored.Collection.Definition.Model); err != nil {
		return err
	}
	body, err := encode(stored)
	if err != nil {
		return err
	}
	collection := stored.Collection
	_, err = db.ExecContext(ctx, "INSERT INTO knowledge_collections(id,revision,generation,body) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,generation=excluded.generation,body=excluded.body",
		collection.ID.String(), int64(collection.Revision), int64(collection.Progress.Generation), body)
	return err
}

// advanceKnowledge starts a new revision whose index is built from scratch.
func advanceKnowledge(stored *KnowledgeStored) error {
	revision, err := nextKnowledge(stored.Collection.Revision)
	if err != nil {
		return err
	}
	generation, err := nextKnowledge(stored.Collection.Progress.Generation)
	if err != nil {
		return err
	}
	stored.Collection.Revision = revision
	stored.Collection.Progress = protocol.DefaultKnowledgeProgress()
	stored.Collection.Progress.Generation = generation
	return nil
}

func requiredKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, id protocol.KnowledgeID, revision uint64) (KnowledgeStored, error) {
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return KnowledgeStored{}, err
	}
	index := slices.IndexFunc(entries, func(stored KnowledgeStored) bool { return stored.Collection.ID == id })
	if index < 0 {
		return KnowledgeStored{}, fault(protocol.ErrorCodeNotFound, "knowledge collection does not exist")
	}
	stored := entries[index]
	if scope := request.Plugin; scope != nil {
		if stored.Collection.Owner.Kind != "plugin" || knowledgeOwner(stored.Collection.Owner) != scope.Package.Name {
			return KnowledgeStored{}, knowledgeDenied("knowledge collection belongs to another owner")
		}
		info, err := knowledgePackage(ctx, db, *scope)
		if err != nil {
			return KnowledgeStored{}, err
		}
		if !knowledgeVisible(stored, request) || !knowledgeReadable(stored, info) {
			return KnowledgeStored{}, knowledgeDenied("knowledge collection is outside the captured scope")
		}
		reference := scope.Package
		stored.Capture.Package = &reference
	}
	return stored, checkRevision(stored.Collection.Revision, revision)
}

func knowledgeChanged(stored KnowledgeStored) (protocol.Output, []protocol.Event, error) {
	return protocol.Output{Kind: "knowledge_collection", Data: stored.Collection}, []protocol.Event{{Kind: "knowledge_collections_changed"}}, nil
}

func createKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, definition protocol.KnowledgeDefinition, selected *string) (protocol.Output, []protocol.Event, error) {
	if request.Plugin != nil && len(definition.SharedWith) != 0 {
		return protocol.Output{}, nil, knowledgeDenied("collection sharing requires a controller")
	}
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if knowledgeManual(entries) >= MaxKnowledgeCollections {
		// Collections of removed owners leave with the next indexing pass;
		// they never use up the capacity.
		if entries, _, err = RemoveOrphanedKnowledge(ctx, db); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	if knowledgeManual(entries) >= MaxKnowledgeCollections {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "knowledge collection capacity exhausted")
	}
	owner, capture, err := knowledgeOwnership(ctx, db, request, selected)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err := validateKnowledge(ctx, db, request, definition, &capture); err != nil {
		return protocol.Output{}, nil, err
	}
	collection := protocol.DefaultKnowledgeCollection()
	collection.ID, collection.Revision, collection.Owner, collection.Definition = protocol.NewKnowledgeID(), 1, owner, definition
	collection.Progress.Generation = 1
	stored := KnowledgeStored{Collection: collection, Capture: capture}
	if err := persistKnowledge(ctx, db, stored); err != nil {
		return protocol.Output{}, nil, err
	}
	return knowledgeChanged(stored)
}

func updateKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, id protocol.KnowledgeID, revision uint64, definition protocol.KnowledgeDefinition) (protocol.Output, []protocol.Event, error) {
	stored, err := requiredKnowledge(ctx, db, request, id, revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if request.Plugin != nil && !slices.Equal(stored.Collection.Definition.SharedWith, definition.SharedWith) {
		return protocol.Output{}, nil, knowledgeDenied("collection sharing requires a controller")
	}
	if err := validateKnowledge(ctx, db, request, definition, &stored.Capture); err != nil {
		return protocol.Output{}, nil, err
	}
	stored.Collection.Definition = definition
	if err := advanceKnowledge(&stored); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := persistKnowledge(ctx, db, stored); err != nil {
		return protocol.Output{}, nil, err
	}
	return knowledgeChanged(stored)
}

func rebuildKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, id protocol.KnowledgeID, revision uint64) (protocol.Output, []protocol.Event, error) {
	stored, err := requiredKnowledge(ctx, db, request, id, revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err := advanceKnowledge(&stored); err != nil {
		return protocol.Output{}, nil, err
	}
	if stored.Collection.Rebuild, err = nextKnowledge(stored.Collection.Rebuild); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := persistKnowledge(ctx, db, stored); err != nil {
		return protocol.Output{}, nil, err
	}
	return knowledgeChanged(stored)
}

func pauseKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, id protocol.KnowledgeID, revision uint64, paused bool) (protocol.Output, []protocol.Event, error) {
	stored, err := requiredKnowledge(ctx, db, request, id, revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	collection := &stored.Collection
	if collection.Paused == paused {
		return protocol.Output{Kind: "knowledge_collection", Data: stored.Collection}, nil, nil
	}
	if collection.Revision, err = nextKnowledge(collection.Revision); err != nil {
		return protocol.Output{}, nil, err
	}
	if collection.Progress.Generation, err = nextKnowledge(collection.Progress.Generation); err != nil {
		return protocol.Output{}, nil, err
	}
	collection.Paused = paused
	collection.Progress.Status = protocol.KnowledgeStatusPending
	if paused {
		collection.Progress.Status = protocol.KnowledgeStatusPaused
		collection.Progress.Model = nil
	}
	if err := persistKnowledge(ctx, db, stored); err != nil {
		return protocol.Output{}, nil, err
	}
	return knowledgeChanged(stored)
}

func removeKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request, id protocol.KnowledgeID, revision uint64) (protocol.Output, []protocol.Event, error) {
	stored, err := requiredKnowledge(ctx, db, request, id, revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "DELETE FROM knowledge_collections WHERE id=?1", stored.Collection.ID.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "knowledge_collection_deleted", Data: protocol.OutputKnowledgeCollectionDeletedPayload{ID: id}},
		[]protocol.Event{{Kind: "knowledge_collections_changed"}}, nil
}

// knowledgeOrphaned reports a collection whose owner no longer exists: the
// worktree it indexes, the conversation whose values it indexes or the
// package that owns it. A disabled package still owns its collections.
func knowledgeOrphaned(ctx context.Context, db *sql.Conn, stored KnowledgeStored) (bool, error) {
	present := true
	switch source := ReadKnowledgeSource(stored.Collection.Definition.Source); {
	case source.Kind == "worktree":
		// A scratch worktree lives as long as a conversation uses it.
		if err := db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM worktrees w WHERE w.id=?1
			AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id)
			AND NOT EXISTS(SELECT 1 FROM removed_projects r WHERE r.project=w.project)
			AND (w.project IS NOT NULL OR EXISTS(SELECT 1 FROM sessions s WHERE s.worktree=w.id AND s.state!='removed')))`, source.Worktree.String()).Scan(&present); err != nil {
			return false, err
		}
	case source.Kind == "storage" && source.Scope == protocol.PluginStorageScopeConversation:
		if stored.Capture.Session == nil {
			return true, nil
		}
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1 AND state!='removed')", stored.Capture.Session.String()).Scan(&present); err != nil {
			return false, err
		}
	}
	if present && stored.Collection.Owner.Kind == "plugin" {
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugins WHERE name=?1 AND installed=1)", knowledgeOwner(stored.Collection.Owner)).Scan(&present); err != nil {
			return false, err
		}
	}
	return !present, nil
}

// OrphanedKnowledge reports whether any collection of a catalog lost its
// owner.
func OrphanedKnowledge(ctx context.Context, db *sql.Conn, catalog []KnowledgeStored) (bool, error) {
	for _, stored := range catalog {
		if orphaned, err := knowledgeOrphaned(ctx, db, stored); err != nil || orphaned {
			return orphaned, err
		}
	}
	return false, nil
}

// RemoveOrphanedKnowledge deletes the collections whose owner was removed
// and returns the remaining catalog. Their derived index goes with the next
// indexing pass. Registering the same worktree, project or package again
// does not bring a collection back.
func RemoveOrphanedKnowledge(ctx context.Context, db *sql.Conn) ([]KnowledgeStored, bool, error) {
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return nil, false, err
	}
	kept := entries[:0]
	for _, stored := range entries {
		orphaned, err := knowledgeOrphaned(ctx, db, stored)
		if err != nil {
			return nil, false, err
		}
		if !orphaned {
			kept = append(kept, stored)
			continue
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM knowledge_collections WHERE id=?1", stored.Collection.ID.String()); err != nil {
			return nil, false, err
		}
	}
	return kept, len(kept) != len(entries), nil
}
