package service

import (
	"context"
	"slices"
	"sync"

	"github.com/jonboulle/clockwork"
	"lukechampine.com/blake3"

	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/node/knowledge/source"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// batch bounds the texts of one embedding call.
const batch = 4

// Options connects the service to the Node that owns it.
type Options struct {
	// Index is the knowledge database of the profile.
	Index *store.Handle
	// Catalog lists every registered collection.
	Catalog func(ctx context.Context) ([]store.KnowledgeStored, error)
	// Allowed lists the collections a request may read.
	Allowed func(ctx context.Context, request protocol.Request) ([]store.KnowledgeStored, error)
	// Progress records and publishes the progress of one collection.
	Progress func(ctx context.Context, id protocol.KnowledgeID, generation uint64, progress protocol.KnowledgeProgress) error
	// Root resolves the current directory of a captured worktree.
	Root func(ctx context.Context, worktree protocol.WorktreeID) (string, error)
	// Choice is the saved embedding selection.
	Choice func() protocol.ModelsLocalChoice
	// Dimension is the vector size of a registered embedding model.
	Dimension func(model string) (int, bool)
	// Installed lists the embedding models that can run.
	Installed func() (map[string]bool, error)
	// Encode embeds at most four texts. Query texts run ahead of background
	// document work.
	Encode func(ctx context.Context, model string, choice protocol.ModelsLocalChoice, texts []string, query bool) ([][]float32, error)
	// Offsets returns the byte range of every model token of a text.
	Offsets func(ctx context.Context, model string, choice protocol.ModelsLocalChoice, text string) ([][2]int, error)
	// OCR reads images and scanned PDF pages.
	OCR   *source.OCR
	Clock clockwork.Clock
	// Working, when set, brackets a collection from its first changed source
	// to the end of its pass.
	Working func() func()
}

// Service owns the retrieval index and the worker that maintains it.
type Service struct {
	options Options
	index   Index
	wake    chan struct{}
	stop    context.Context
	halt    context.CancelFunc
	done    chan struct{}
	started bool

	mu     sync.Mutex
	pass   context.Context
	cancel context.CancelFunc
}

// New prepares a service; Start runs its worker.
func New(options Options) *Service {
	service := &Service{options: options, index: Index{options.Index}, wake: make(chan struct{}, 1), done: make(chan struct{})}
	service.stop, service.halt = context.WithCancel(context.Background())
	service.pass, service.cancel = context.WithCancel(service.stop)
	return service
}

// Start runs the indexing worker and requests the first pass.
func (service *Service) Start() {
	service.started = true
	go service.run()
	service.Refresh()
}

// Close cancels indexing and waits for a started worker.
func (service *Service) Close() {
	service.halt()
	if service.started {
		<-service.done
	}
}

// Refresh asks the worker for another pass.
func (service *Service) Refresh() {
	select {
	case service.wake <- struct{}{}:
	default:
	}
}

// Invalidate cancels the running pass and asks for another one.
func (service *Service) Invalidate() {
	service.mu.Lock()
	service.cancel()
	service.pass, service.cancel = context.WithCancel(service.stop)
	service.mu.Unlock()
	service.Refresh()
}

func unavailable() error {
	return newFault(protocol.ErrorCodeUnavailable, "knowledge service is unavailable")
}

// binding captures what a collection is indexed for. The model is the
// collection's own or the selected one, when it is registered.
func (service *Service) binding(stored store.KnowledgeStored, choice protocol.ModelsLocalChoice) Binding {
	collection := stored.Collection
	var name *string
	if stored.Capture.Package != nil {
		name = &stored.Capture.Package.Name
	}
	binding := Binding{
		ID:         collection.ID,
		Generation: collection.Progress.Generation,
		Scope: blake3.Sum256([]byte(encoded([]any{
			collection.Owner, collection.Definition.Source, collection.Definition.Globs, collection.Definition.Tags,
			name, stored.Capture.Worktree, stored.Capture.Session, stored.Automatic,
		}))),
		Rebuild: collection.Rebuild,
		Paused:  collection.Paused,
	}
	model := ""
	if collection.Definition.Model != nil {
		model = *collection.Definition.Model
	} else if choice.Enabled && choice.Model != nil {
		model = *choice.Model
	}
	if model != "" {
		if dimension, found := service.options.Dimension(model); found {
			binding.Model, binding.Dimension = model, dimension
		}
	}
	return binding
}

func nonzero(vector []float32) bool {
	return slices.ContainsFunc(vector, func(value float32) bool { return value != 0 })
}

// Query answers a search with the collections the request may read.
func (service *Service) Query(ctx context.Context, request protocol.Request, query protocol.KnowledgeQuery) (protocol.KnowledgePage, error) {
	if err := knowledge.ValidateQuery(query); err != nil {
		return protocol.DefaultKnowledgePage(), err
	}
	allowed, err := service.options.Allowed(ctx, request)
	if err != nil {
		return protocol.DefaultKnowledgePage(), err
	}
	choice := service.options.Choice()
	var bindings []Binding
	var models []string
	indexing := false
	for _, stored := range allowed {
		if len(query.Collections) != 0 && !slices.Contains(query.Collections, stored.Collection.ID) {
			continue
		}
		binding := service.binding(stored, choice)
		bindings = append(bindings, binding)
		status := stored.Collection.Progress.Status
		indexing = indexing || status == protocol.KnowledgeStatusPending || status == protocol.KnowledgeStatusIndexing
		if !stored.Collection.Paused && binding.Model != "" && !slices.Contains(models, binding.Model) {
			models = append(models, binding.Model)
		}
	}
	slices.Sort(models)
	var embeddings []Embedding
	if query.Mode != protocol.KnowledgeModeKeyword {
		for _, model := range models {
			vectors, err := service.options.Encode(ctx, model, choice, []string{query.Query}, true)
			if err != nil {
				if code(err) == protocol.ErrorCodeNotConfigured {
					continue
				}
				return protocol.DefaultKnowledgePage(), err
			}
			if len(vectors) != 1 {
				return protocol.DefaultKnowledgePage(), newFault(protocol.ErrorCodeInternal, "embedding query count differs")
			}
			if nonzero(vectors[0]) {
				embeddings = append(embeddings, Embedding{Model: model, Values: vectors[0]})
			}
		}
	}
	return service.index.Search(ctx, query, bindings, embeddings, indexing)
}
