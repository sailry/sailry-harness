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

// initKnowledge starts the retrieval service over the profile's knowledge
// database. Embedding and OCR run through the local model scheduler.
func (node *Node) initKnowledge() {
	registry := node.localModels.registry
	node.knowledge = knowledgeservice.New(knowledgeservice.Options{
		Working: node.power.Hold,
		Index:   node.profile.Store.Knowledge,
		Catalog: func(ctx context.Context) (catalog []store.KnowledgeStored, err error) {
			err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
				catalog, err = store.ReadKnowledgeCatalog(ctx, db)
				return err
			})
			return catalog, nodeFault(err)
		},
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
		Dimension: func(model string) (int, bool) {
			definition, err := registry.Definition(model)
			if err != nil {
				return 0, false
			}
			dimension, found := localmodels.Dimension(definition.Format)
			return int(dimension), found
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
				resource, err := node.ocrResource()
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
		Clock: clockwork.NewRealClock(),
	})
	node.knowledge.Start()
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
	same := func(left, right protocol.ModelsLocalChoice) bool {
		return left.Enabled == right.Enabled && (left.Model == nil) == (right.Model == nil) && (left.Model == nil || *left.Model == *right.Model)
	}
	switch {
	case !same(previous.Embedding, latest.Embedding) ||
		!slices.Equal(installed(previous, protocol.ModelsKindEmbedding), installed(latest, protocol.ModelsKindEmbedding)):
		node.knowledge.Invalidate()
	case !same(previous.OCR, latest.OCR) ||
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
		completion <- response(protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "knowledge collection result could not be recorded; list the collections before retrying"})
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
