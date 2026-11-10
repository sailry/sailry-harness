package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// MaxKnowledgeCollections bounds the collections of one Node.
const MaxKnowledgeCollections = 128

// KnowledgeCapture is the plugin scope a collection was created in.
type KnowledgeCapture struct {
	Package  *protocol.PluginReference `json:"package"`
	Worktree *protocol.WorktreeID      `json:"worktree"`
	Session  *protocol.SessionID       `json:"session"`
}

// KnowledgeStored is one durable collection with its captured scope. The
// retrieval index is derived from it.
type KnowledgeStored struct {
	Collection protocol.KnowledgeCollection `json:"collection"`
	Capture    KnowledgeCapture             `json:"capture"`
	Automatic  bool                         `json:"automatic"`
}

// KnowledgeSource is the flat reading of a source union.
type KnowledgeSource struct {
	Kind     string                      `json:"kind"`
	Worktree protocol.WorktreeID         `json:"worktree"`
	Path     string                      `json:"path"`
	Scope    protocol.PluginStorageScope `json:"scope"`
	Prefix   string                      `json:"prefix"`
}

// ReadKnowledgeSource flattens a validated source union.
func ReadKnowledgeSource(source protocol.KnowledgeSource) KnowledgeSource {
	var flat KnowledgeSource
	if body, err := json.Marshal(source); err == nil {
		_ = json.Unmarshal(body, &flat)
	}
	return flat
}

func knowledgeOwner(owner protocol.KnowledgeOwner) string {
	var flat struct {
		Name string `json:"name"`
	}
	if body, err := json.Marshal(owner); err == nil {
		_ = json.Unmarshal(body, &flat)
	}
	return flat.Name
}

func sameJSON(left, right any) bool {
	first, err := json.Marshal(left)
	if err != nil {
		return false
	}
	second, err := json.Marshal(right)
	return err == nil && bytes.Equal(first, second)
}

// SameKnowledgeStored compares the identity an indexing pass or a citation
// read captured with the current collection.
func SameKnowledgeStored(left, right KnowledgeStored) bool {
	return left.Collection.ID == right.Collection.ID && left.Collection.Revision == right.Collection.Revision &&
		left.Collection.Rebuild == right.Collection.Rebuild && left.Collection.Paused == right.Collection.Paused &&
		left.Collection.Progress.Generation == right.Collection.Progress.Generation && left.Automatic == right.Automatic &&
		sameJSON(left.Collection.Owner, right.Collection.Owner) && sameJSON(left.Collection.Definition, right.Collection.Definition) &&
		sameJSON(left.Capture, right.Capture)
}

func nextKnowledge(value uint64) (uint64, error) {
	if value >= math.MaxInt64 {
		return 0, invalid("knowledge revision is exhausted")
	}
	return value + 1, nil
}

func knowledgeDenied(message string) error {
	return fault(protocol.ErrorCodePermissionDenied, message)
}

// ReadKnowledgeCatalog reads every stored collection for the indexing worker.
func ReadKnowledgeCatalog(ctx context.Context, db *sql.Conn) ([]KnowledgeStored, error) {
	return loadKnowledge(ctx, db)
}

// AllowedKnowledge returns the collections a request may read. Query plans
// intersect these durable grants before reading the derived index.
func AllowedKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request) ([]KnowledgeStored, error) {
	var info *protocol.PluginInfo
	if request.Plugin != nil {
		captured, err := knowledgePackage(ctx, db, *request.Plugin)
		if err != nil {
			return nil, err
		}
		info = &captured
	}
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return nil, err
	}
	return slices.DeleteFunc(entries, func(stored KnowledgeStored) bool {
		return !knowledgeVisible(stored, request) || !knowledgeGranted(stored, request, info)
	}), nil
}

// ListKnowledgeCollections lists the collections a request may read.
func ListKnowledgeCollections(ctx context.Context, db *sql.Conn, request protocol.Request) ([]protocol.KnowledgeCollection, error) {
	entries, err := AllowedKnowledge(ctx, db, request)
	if err != nil {
		return nil, err
	}
	collections := make([]protocol.KnowledgeCollection, 0, len(entries))
	for _, stored := range entries {
		collections = append(collections, stored.Collection)
	}
	return collections, nil
}

func knowledgeGranted(stored KnowledgeStored, request protocol.Request, info *protocol.PluginInfo) bool {
	if request.Plugin != nil && info != nil && stored.Collection.Owner.Kind == "plugin" && knowledgeOwner(stored.Collection.Owner) == request.Plugin.Package.Name {
		return knowledgeReadable(stored, *info)
	}
	return true
}

func knowledgeVisible(stored KnowledgeStored, request protocol.Request) bool {
	scope := request.Plugin
	if scope == nil {
		return true
	}
	collection := stored.Collection
	if collection.Owner.Kind != "plugin" || knowledgeOwner(collection.Owner) != scope.Package.Name {
		return slices.Contains(collection.Definition.SharedWith, scope.Package.Name)
	}
	source := ReadKnowledgeSource(collection.Definition.Source)
	switch {
	case source.Kind == "worktree":
		return scope.Worktree != nil && *scope.Worktree == source.Worktree
	case source.Kind == "storage" && source.Scope == protocol.PluginStorageScopeConversation:
		return scope.Session != nil && stored.Capture.Session != nil && *stored.Capture.Session == *scope.Session
	}
	return true
}

// ExecuteKnowledge applies one collection mutation.
func ExecuteKnowledge(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.Output, []protocol.Event, error) {
	switch input := request.Command.Data.(type) {
	case *protocol.CommandCreateKnowledgeCollectionPayload:
		return createKnowledge(ctx, db, request, input.Definition, input.Plugin)
	case *protocol.CommandUpdateKnowledgeCollectionPayload:
		return updateKnowledge(ctx, db, request, input.ID, input.ExpectedRevision, input.Definition)
	case *protocol.CommandDeleteKnowledgeCollectionPayload:
		return removeKnowledge(ctx, db, request, input.ID, input.ExpectedRevision)
	case *protocol.CommandRebuildKnowledgeCollectionPayload:
		return rebuildKnowledge(ctx, db, request, input.ID, input.ExpectedRevision)
	case *protocol.CommandPauseKnowledgeCollectionPayload:
		return pauseKnowledge(ctx, db, request, input.ID, input.ExpectedRevision, input.Paused)
	}
	return protocol.Output{}, nil, invalid("knowledge mutation expected")
}

// RecordKnowledgeProgress stores the worker's progress for the generation
// it indexed. It reports whether the collection changed.
func RecordKnowledgeProgress(ctx context.Context, db *sql.Conn, id protocol.KnowledgeID, generation uint64, progress protocol.KnowledgeProgress) (bool, error) {
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return false, err
	}
	index := slices.IndexFunc(entries, func(stored KnowledgeStored) bool { return stored.Collection.ID == id })
	if index < 0 {
		return false, fault(protocol.ErrorCodeNotFound, "knowledge collection no longer exists")
	}
	stored := entries[index]
	if stored.Collection.Progress.Generation != generation || progress.Generation != generation {
		return false, fault(protocol.ErrorCodeRevisionConflict, "knowledge collection generation changed")
	}
	model := stored.Collection.Definition.Model
	if model == nil {
		settings, err := ReadLocalModelSettings(ctx, db)
		if err != nil {
			return false, err
		}
		if selected := localmodels.Selected(&settings, protocol.ModelsKindEmbedding); selected != "" {
			model = &selected
		}
	}
	if progress.Model != nil && (model == nil || *progress.Model != *model) {
		return false, fault(protocol.ErrorCodeRevisionConflict, "knowledge collection model changed")
	}
	if sameJSON(stored.Collection.Progress, progress) {
		return false, nil
	}
	stored.Collection.Progress = progress
	return true, persistKnowledge(ctx, db, stored)
}

// knowledgeSelectionChanged restarts indexing of every collection that
// follows the default embedding model.
func knowledgeSelectionChanged(ctx context.Context, db *sql.Conn) error {
	entries, err := loadKnowledge(ctx, db)
	if err != nil {
		return err
	}
	for _, stored := range entries {
		if stored.Collection.Definition.Model != nil {
			continue
		}
		if err := advanceKnowledge(&stored); err != nil {
			return err
		}
		if err := persistKnowledge(ctx, db, stored); err != nil {
			return err
		}
	}
	return nil
}
