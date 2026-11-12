package node

import (
	"context"
	"database/sql"
	"slices"

	"github.com/jonboulle/clockwork"
	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/embedding"
	"github.com/sailry/sailry-harness/internal/node/admission"
	knowledgeservice "github.com/sailry/sailry-harness/internal/node/knowledge/service"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func knowledgeCommand(kind string) bool {
	switch kind {
	case "query_knowledge", "read_knowledge_citation", "list_knowledge_collections",
		"create_knowledge_collection", "update_knowledge_collection", "delete_knowledge_collection",
		"rebuild_knowledge_collection", "pause_knowledge_collection":
		return true
	}
	return false
}

// initKnowledge registers the semantic storage collections of the enabled
// packages and starts the retrieval service.
func (node *Node) initKnowledge(ctx context.Context) error {
	changed := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		if changed, err = store.RegisterKnowledgeSemantic(ctx, db); err != nil || !changed {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "knowledge_collections_changed"}})
		return err
	})
	if err != nil {
		return err
	}
	options := node.knowledgeOptions()
	if node.knowledgeAdjust != nil {
		node.knowledgeAdjust(&options)
	}
	node.knowledge = knowledgeservice.New(options)
	node.knowledge.Start()
	return nil
}

// followKnowledgeStorage keeps the semantic storage collections current in
// the transaction of a plugin command: a changed package registers its
// declared collections, and a storage write restarts the indexing of the
// writer's collections.
func (node *Node) followKnowledgeStorage(ctx context.Context, db *sql.Conn, request protocol.Request, changes []protocol.Event) ([]protocol.Event, error) {
	changed := false
	if slices.ContainsFunc(changes, func(change protocol.Event) bool { return change.Kind == "plugin_changed" }) {
		registered, err := store.RegisterKnowledgeSemantic(ctx, db)
		if err != nil {
			return changes, err
		}
		changed = registered
	}
	if store.KnowledgeStorageCommand(request.Command) {
		advanced, err := store.KnowledgeStorageChanged(ctx, db, request)
		if err != nil {
			return changes, err
		}
		changed = changed || advanced
	}
	if changed {
		changes = append(changes, protocol.Event{Kind: "knowledge_collections_changed"})
	}
	return changes, nil
}

// refreshKnowledgeStorage asks for an indexing pass after a committed
// command that may change what storage collections index, that registered
// one, or that removed an owner of collections. The pass runs on the knowledge worker and never delays the
// command.
func (node *Node) refreshKnowledgeStorage(command protocol.Command, changes []protocol.Event) {
	switch command.Kind {
	case "install_plugin", "install_plugin_source", "install_plugin_upload", "set_plugin_enabled", "save_plugin_settings":
	case "remove_plugin", "remove_project", "remove_session", "remove_sessions":
		// The pass removes the collections these owned.
	default:
		if !store.KnowledgeStorageCommand(command) && !slices.ContainsFunc(changes, func(change protocol.Event) bool { return change.Kind == "knowledge_collections_changed" }) {
			return
		}
	}
	node.knowledge.Refresh()
}

// knowledgeOptions connects the retrieval service to the profile. Embedding
// and OCR run through the local model scheduler.
func (node *Node) knowledgeOptions() knowledgeservice.Options {
	registry := node.localModels.registry
	return knowledgeservice.Options{
		Working: node.power.Hold,
		Index:   node.profile.Store.Knowledge,
		Catalog: node.knowledgeCatalog,
		Allowed: func(ctx context.Context, request protocol.Request) (allowed []store.KnowledgeStored, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				allowed, err = store.AllowedKnowledge(ctx, db, request)
				return err
			})
			return allowed, nodeFault(err)
		},
		Progress: node.recordKnowledgeProgress,
		Root: func(ctx context.Context, worktree protocol.WorktreeID) (string, error) {
			root, err := node.workspaceRoot(ctx, worktree, false)
			return root, nodeFault(err)
		},
		Choice: func() protocol.ModelsLocalChoice {
			settings := registry.Settings()
			return localmodels.Choice(&settings, protocol.ModelsKindEmbedding)
		},
		Dimension: func(model string) (int, string, bool) {
			definition, err := registry.Definition(model)
			if err != nil {
				return 0, "", false
			}
			// A pinned model keeps its name when its revision moves; the
			// definition's identity tells its vectors apart.
			dimension, found := localmodels.Dimension(definition.Format)
			return int(dimension), localmodels.ID(definition), found
		},
		Installed: func() (map[string]bool, error) {
			models, err := registry.List(protocol.ModelsKindEmbedding)
			if err != nil {
				return nil, err
			}
			installed := map[string]bool{}
			for _, model := range models {
				installed[model.ID] = model.State.Kind == "installed"
			}
			return installed, nil
		},
		Encode: func(ctx context.Context, model string, choice protocol.ModelsLocalChoice, texts []string, query bool) ([][]float32, error) {
			if len(texts) > 4 {
				return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "embedding batch exceeds four inputs"}
			}
			err := registry.Settle(ctx, model)
			if err != nil {
				return nil, err
			}
			resource, err := registry.Resource(protocol.ModelsKindEmbedding, model, &choice)
			if err != nil {
				return nil, err
			}
			input, lane := embedding.Document, localmodels.Background
			if query {
				input, lane = embedding.Query, localmodels.Interactive
			}
			return localmodels.Execute(ctx, registry, resource, lane, func(_ context.Context, engine any) ([][]float32, error) {
				vectors, err := engine.(*embedding.Engine).Encode(input, texts)
				return vectors, embeddingFailure(err)
			})
		},
		Offsets: func(ctx context.Context, model string, choice protocol.ModelsLocalChoice, text string) ([][2]int, error) {
			err := registry.Settle(ctx, model)
			if err != nil {
				return nil, err
			}
			resource, err := registry.Resource(protocol.ModelsKindEmbedding, model, &choice)
			if err != nil {
				return nil, err
			}
			return localmodels.Execute(ctx, registry, resource, localmodels.Background, func(_ context.Context, engine any) ([][2]int, error) {
				offsets, err := engine.(*embedding.Engine).Offsets(text)
				return offsets, embeddingFailure(err)
			})
		},
		OCR: &source.OCR{
			Signature: func() ([32]byte, error) {
				resource, err := node.ocrResource(node.ctx)
				if notConfigured(err) {
					return [32]byte{}, nil
				}
				if err != nil {
					return [32]byte{}, err
				}
				return blake3.Sum256([]byte(resource.ID)), nil
			},
			Parts: func(ctx context.Context, name string, data []byte) ([]office.Part, error) {
				return node.documentParts(ctx, name, data, localmodels.Background)
			},
		},
		Sources: func(ctx context.Context, stored store.KnowledgeStored, after *string) (page store.KnowledgeStoragePage, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				page, err = store.ReadKnowledgeStoragePage(ctx, db, stored, after, store.KnowledgeStoragePageLimit)
				return err
			})
			return page, nodeFault(err)
		},
		Source: func(ctx context.Context, stored store.KnowledgeStored, key string) (record *store.KnowledgeStorageRecord, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				record, err = store.ReadKnowledgeStorageRecord(ctx, db, stored, key)
				return err
			})
			return record, nodeFault(err)
		},
		SourceCurrent: func(ctx context.Context, stored store.KnowledgeStored, record store.KnowledgeStorageRecord) (current bool, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				current, err = store.KnowledgeStorageUnchanged(ctx, db, stored, record)
				return err
			})
			return current, nodeFault(err)
		},
		Plan: func(ctx context.Context, request protocol.Request) (plan store.SemanticPlan, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				plan, err = store.PrepareSemanticSearch(ctx, db, request)
				return err
			})
			return plan, nodeFault(err)
		},
		Materialize: func(ctx context.Context, request protocol.Request, plan store.SemanticPlan, selection store.SemanticSelection) (page protocol.PluginStorageSearchPage, stale bool, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				page, stale, err = store.MaterializeSemanticSearch(ctx, db, request, plan, selection)
				return err
			})
			return page, stale, nodeFault(err)
		},
		Clock: clockwork.NewRealClock(),
	}
}

// knowledgeCatalog reads the collections for an indexing pass after removing
// those whose worktree, project, conversation or package was removed. Every
// pass reconciles ownership, so a removal needs no hook of its own beyond
// requesting a pass.
func (node *Node) knowledgeCatalog(ctx context.Context) ([]store.KnowledgeStored, error) {
	var catalog []store.KnowledgeStored
	orphaned := false
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		if catalog, err = store.ReadKnowledgeCatalog(ctx, db); err == nil {
			orphaned, err = store.OrphanedKnowledge(ctx, db, catalog)
		}
		return err
	})
	if err != nil || !orphaned {
		return catalog, nodeFault(err)
	}
	removed := false
	err = node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		if catalog, removed, err = store.RemoveOrphanedKnowledge(ctx, db); err != nil || !removed {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "knowledge_collections_changed"}})
		return err
	})
	if err == nil && removed {
		node.publish()
	}
	return catalog, nodeFault(err)
}

func embeddingFailure(err error) error {
	if err == nil {
		return nil
	}
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "embedding failed: " + err.Error()}
}

// recordKnowledgeProgress stores the worker's progress and announces a
// changed collection.
func (node *Node) recordKnowledgeProgress(ctx context.Context, id protocol.KnowledgeID, generation uint64, progress protocol.KnowledgeProgress) error {
	changed := false
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		if changed, err = store.RecordKnowledgeProgress(ctx, db, id, generation, progress); err != nil || !changed {
			return err
		}
		_, err = store.RecordEvents(ctx, db, node.ID(), []protocol.Event{{Kind: "knowledge_collections_changed"}})
		return err
	})
	if err == nil && changed {
		node.publish()
	}
	return nodeFault(err)
}

// reactKnowledge follows the local model registry: a changed embedding
// selection or installed embedding set restarts indexing, and a changed OCR
// selection or installed OCR set rescans.
func (node *Node) reactKnowledge(previous, latest localmodels.Update) {
	installed := func(update localmodels.Update, kind protocol.ModelsKind) []string {
		var ids []string
		for _, model := range update.Models {
			if localmodels.Kind(model.Definition.Format) == kind && model.State.Kind == "installed" {
				ids = append(ids, model.ID)
			}
		}
		slices.Sort(ids)
		return ids
	}
	switch {
	case !localmodels.SameChoice(previous.Embedding, latest.Embedding) ||
		!slices.Equal(installed(previous, protocol.ModelsKindEmbedding), installed(latest, protocol.ModelsKindEmbedding)):
		node.knowledge.Invalidate()
	case !localmodels.SameChoice(previous.OCR, latest.OCR) ||
		!slices.Equal(installed(previous, protocol.ModelsKindOcr), installed(latest, protocol.ModelsKindOcr)):
		node.knowledge.Refresh()
	}
}

func (node *Node) dispatchKnowledge(ctx context.Context, caller protocol.NodeID, request protocol.Request, body []byte) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	accepted := link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}
	if !accepted.Receipt.Durable {
		go func() {
			defer node.work.Done()
			defer close(completion)
			// Retrieval stops with its caller and with the Node.
			operation, cancel := context.WithCancel(ctx)
			defer cancel()
			defer context.AfterFunc(node.ctx, cancel)()
			completion <- response(node.readKnowledge(operation, request))
		}()
		return accepted, nil
	}
	defer node.work.Done()
	var previous *link.Response
	err := node.profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		var err error
		previous, err = admission.Admit(ctx, db, caller, request, body)
		return err
	})
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	defer close(completion)
	if previous != nil {
		completion <- *previous
		return accepted, nil
	}
	var output protocol.Output
	var operationErr error
	err = node.profile.Store.Node.Write(node.ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(node.ctx, "SAVEPOINT sailry_knowledge"); err != nil {
			return err
		}
		var events []protocol.Event
		if operationErr = node.validatePluginRequestDB(node.ctx, db, caller, request); operationErr == nil {
			output, events, operationErr = store.ExecuteKnowledge(node.ctx, db, request)
		}
		if operationErr != nil {
			events = nil
			if _, err := db.ExecContext(node.ctx, "ROLLBACK TO sailry_knowledge"); err != nil {
				return err
			}
		}
		if _, err := db.ExecContext(node.ctx, "RELEASE sailry_knowledge"); err != nil {
			return err
		}
		if _, err := store.RecordEvents(node.ctx, db, node.ID(), events); err != nil {
			return err
		}
		return admission.Complete(node.ctx, db, caller, request.ID, response(output, operationErr))
	})
	if err != nil {
		completion <- node.unapplied(caller, request.ID, err)
		return accepted, nil
	}
	if operationErr == nil {
		node.knowledge.Invalidate()
		node.publish()
	}
	completion <- response(output, operationErr)
	return accepted, nil
}

func (node *Node) readKnowledge(ctx context.Context, request protocol.Request) (protocol.Output, error) {
	switch input := request.Command.Data.(type) {
	case *protocol.KnowledgeQuery:
		page, err := node.knowledge.Query(ctx, request, *input)
		return protocol.Output{Kind: "knowledge_search", Data: page}, err
	case *protocol.KnowledgeCitationRequest:
		page, err := node.knowledge.Citation(ctx, request, *input)
		return protocol.Output{Kind: "knowledge_citation", Data: page}, err
	}
	if request.Command.Kind != "list_knowledge_collections" {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "knowledge read expected"}
	}
	collections := []protocol.KnowledgeCollection{}
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		collections, err = store.ListKnowledgeCollections(ctx, db, request)
		return err
	})
	return protocol.Output{Kind: "knowledge_collections", Data: collections}, err
}
