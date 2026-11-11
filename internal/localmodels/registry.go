package localmodels

import (
	"context"
	"encoding/json"
	"maps"
	"net/http"
	"slices"
	"strings"
	"sync"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Options configure one Node's registry.
type Options struct {
	// Profile is the Node's data directory. Empty leaves model storage
	// unavailable.
	Profile string
	// Capabilities are the inference runtimes present in this build.
	Capabilities Capabilities
	// Transport performs asset downloads and carries the Node's proxy settings.
	Transport http.RoundTripper
	// Clock drives idle release. Nil uses the system clock.
	Clock clockwork.Clock
}

// Registry owns model identities, verified installations and configuration
// for one Node.
//
// Verified stamps are the installed-state authority. Removal clears a stamp
// before deleting files, so queued captures fail without a separate
// generation counter.
type Registry struct {
	profile      string
	capabilities Capabilities
	client       *http.Client
	scheduler    *scheduler
	clock        clockwork.Clock
	stop         context.Context
	cancel       context.CancelFunc

	rootMu sync.Mutex
	root   string
	// inspecting serializes hashing inspections so concurrent listings do
	// not hash the same files. Start's receipt pass does not take it.
	inspecting sync.Mutex

	tasksMu sync.Mutex
	closed  bool
	tasks   sync.WaitGroup

	mu          sync.Mutex
	settings    protocol.ModelsLocalSettings
	definitions map[string]*protocol.ModelsDefinition
	verified    map[string]*assets.Stamp
	removing    map[string]bool
	// pending holds the models Start found on disk without a receipt; each
	// channel closes when that model's hashing ends.
	pending   map[string]chan struct{}
	transfers map[string]context.CancelFunc
	scopes    map[protocol.ModelsKind]scope
	latest    Update
	signature string
	changed   chan struct{}
}

type scope struct {
	ctx    context.Context
	cancel context.CancelFunc
}

// Prepared is a validated configuration waiting for its durable commit.
type Prepared struct {
	settings    protocol.ModelsLocalSettings
	definitions map[string]*protocol.ModelsDefinition
}

// Update is the published registry state.
type Update struct {
	Models    []protocol.ModelsLocalModel
	Embedding protocol.ModelsLocalChoice
	OCR       protocol.ModelsLocalChoice
}

// New starts a registry holding the pinned builtins and default settings.
func New(options Options) *Registry {
	stop, cancel := context.WithCancel(context.Background())
	clock := options.Clock
	if clock == nil {
		clock = clockwork.NewRealClock()
	}
	settings := protocol.DefaultModelsLocalSettings()
	FillDefaults(&settings)
	registry := &Registry{
		profile:      options.Profile,
		capabilities: options.Capabilities,
		client:       assets.Client(options.Transport),
		scheduler:    newScheduler(stop, clock, options.Capabilities),
		clock:        clock,
		stop:         stop,
		cancel:       cancel,
		settings:     settings,
		definitions:  maps.Clone(builtins().definitions),
		verified:     map[string]*assets.Stamp{},
		removing:     map[string]bool{},
		transfers:    map[string]context.CancelFunc{},
		scopes:       map[protocol.ModelsKind]scope{},
		changed:      make(chan struct{}),
	}
	for _, kind := range NodeKinds {
		registry.scopes[kind] = registry.scope()
	}
	registry.latest, registry.signature = registry.update()
	return registry
}

func (registry *Registry) scope() scope {
	ctx, cancel := context.WithCancel(registry.stop)
	return scope{ctx: ctx, cancel: cancel}
}

// spawn runs background inspection that Shutdown waits for and reports
// whether it was started.
func (registry *Registry) spawn(task func()) bool {
	registry.tasksMu.Lock()
	defer registry.tasksMu.Unlock()
	if !registry.closed {
		registry.tasks.Go(task)
	}
	return !registry.closed
}

// Capabilities are the inference runtimes this registry schedules.
func (registry *Registry) Capabilities() Capabilities { return registry.capabilities }

// Prepare validates settings against the definitions they will be saved with.
func (registry *Registry) Prepare(settings protocol.ModelsLocalSettings, definitions map[string]*protocol.ModelsDefinition) (*Prepared, error) {
	settings.Choices = maps.Clone(settings.Choices)
	FillDefaults(&settings)
	if err := ValidateSettings(&settings); err != nil {
		return nil, err
	}
	for kind, choice := range settings.Choices {
		if choice.Model == nil {
			continue
		}
		if definition, registered := definitions[*choice.Model]; !registered || Kind(definition.Format) != kind {
			return nil, invalid("selected local model is not registered for its kind")
		}
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	for id, previous := range registry.definitions {
		if next, kept := definitions[id]; kept && !same(next, previous) {
			return nil, invalid("registered model definition is immutable")
		}
	}
	return &Prepared{settings: settings, definitions: definitions}, nil
}

// Apply publishes a prepared configuration after its durable commit.
func (registry *Registry) Apply(prepared *Prepared) {
	registry.mu.Lock()
	for id, cancel := range registry.transfers {
		if sourceChanged(prepared.definitions[id], registry.settings.Mirror, prepared.settings.Mirror) {
			cancel()
		}
	}
	for _, kind := range NodeKinds {
		if !sameChoice(Choice(&registry.settings, kind), Choice(&prepared.settings, kind)) {
			registry.scopes[kind].cancel()
			registry.scopes[kind] = registry.scope()
		}
	}
	added := false
	for id := range prepared.definitions {
		// A kept identity keeps the definition value captured resources hold.
		if previous, kept := registry.definitions[id]; kept {
			prepared.definitions[id] = previous
		} else {
			added = true
		}
	}
	maps.DeleteFunc(registry.verified, func(id string, _ *assets.Stamp) bool { return prepared.definitions[id] == nil })
	registry.settings, registry.definitions = prepared.settings, prepared.definitions
	registry.publish()
	registry.mu.Unlock()
	if added {
		// Registered files may already exist; inspect them off the caller's thread.
		registry.spawn(func() { _, _ = registry.List("") })
	}
}

// Configure prepares and applies in one step, for callers without a durable
// commit between them.
func (registry *Registry) Configure(settings protocol.ModelsLocalSettings, records []Registration) error {
	definitions, err := Definitions(records)
	if err != nil {
		return err
	}
	prepared, err := registry.Prepare(settings, definitions)
	if err != nil {
		return err
	}
	registry.Apply(prepared)
	return nil
}

// Settings is the applied configuration.
func (registry *Registry) Settings() protocol.ModelsLocalSettings {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	settings := registry.settings
	settings.Choices = maps.Clone(settings.Choices)
	return settings
}

// Updates returns the published state and a channel closed at its next change.
func (registry *Registry) Updates() (Update, <-chan struct{}) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	return registry.latest, registry.changed
}

// List verifies the files of one kind, or of every kind when kind is empty,
// and returns the models.
func (registry *Registry) List(kind protocol.ModelsKind) ([]protocol.ModelsLocalModel, error) {
	if kind == protocol.ModelsKindSpeech {
		return nil, speech()
	}
	if registry.profile != "" {
		err := registry.verify(func(_ string, definition *protocol.ModelsDefinition) bool {
			return kind == "" || Kind(definition.Format) == kind
		})
		if err != nil {
			return nil, err
		}
	}
	return registry.Cached(kind), nil
}

// Start accepts the files their receipts cover without reading contents and
// hashes the others off the caller's thread, so a large model without a
// receipt does not delay startup. Such a model is listed as missing until
// its hash ends; operations that need it wait through Settle. Start also
// sweeps what interrupted downloads left behind and cannot be resumed.
func (registry *Registry) Start() error {
	if registry.profile == "" {
		return nil
	}
	registry.sweep()
	if err := registry.inspect(func(string, *protocol.ModelsDefinition) bool { return true }, known); err != nil {
		return err
	}
	root, err := registry.Root()
	if err != nil {
		return err
	}
	registry.mu.Lock()
	unverified := map[string]*protocol.ModelsDefinition{}
	for id, definition := range registry.definitions {
		if registry.verified[id] == nil {
			unverified[id] = definition
		}
	}
	registry.mu.Unlock()
	pending := map[string]chan struct{}{}
	for id, definition := range unverified {
		// Only marked files can verify; anything else stays missing.
		if path, err := directory(root, id, definition); err == nil && assets.StampOf(path, files(definition), ID(definition)) != nil {
			pending[id] = make(chan struct{})
		}
	}
	registry.mu.Lock()
	registry.pending = pending
	registry.mu.Unlock()
	started := registry.spawn(func() {
		for _, id := range slices.Sorted(maps.Keys(pending)) {
			if registry.stop.Err() == nil {
				_ = registry.verify(func(candidate string, _ *protocol.ModelsDefinition) bool { return candidate == id })
			}
			close(pending[id])
		}
	})
	if !started {
		// A closed registry hashes nothing, so nothing is waited for.
		registry.mu.Lock()
		registry.pending = nil
		registry.mu.Unlock()
	}
	return nil
}

// sweep clears the leftovers of interrupted downloads from the directory of
// every registered model. No transfer runs before Start.
func (registry *Registry) sweep() {
	root, err := registry.Root()
	if err != nil {
		return
	}
	registry.mu.Lock()
	definitions := maps.Clone(registry.definitions)
	registry.mu.Unlock()
	stale := registry.clock.Now().Add(-assets.PartRetention)
	for id, definition := range definitions {
		if path, err := directory(root, id, definition); err == nil {
			assets.Sweep(path, files(definition), stale)
		}
	}
}

// Settle waits until the hashing Start left running for the given models
// has ended, so a model on disk is not reported missing while it is being
// verified.
func (registry *Registry) Settle(ctx context.Context, ids ...string) error {
	registry.mu.Lock()
	var waits []chan struct{}
	for _, id := range ids {
		if done := registry.pending[id]; done != nil {
			waits = append(waits, done)
		}
	}
	registry.mu.Unlock()
	for _, done := range waits {
		select {
		case <-done:
		case <-ctx.Done():
			return Cancelled()
		case <-registry.stop.Done():
			return Cancelled()
		}
	}
	return nil
}

// verify inspects files outside the configuration lock; hashing runs only
// for changed files.
func (registry *Registry) verify(include func(string, *protocol.ModelsDefinition) bool) error {
	registry.inspecting.Lock()
	defer registry.inspecting.Unlock()
	return registry.inspect(include, stamp)
}

func (registry *Registry) inspect(include func(string, *protocol.ModelsDefinition) bool, observe func(string, *protocol.ModelsDefinition, *assets.Stamp) *assets.Stamp) error {
	root, err := registry.Root()
	if err != nil {
		return err
	}
	type observation struct {
		id         string
		definition *protocol.ModelsDefinition
		cached     *assets.Stamp
		stamp      *assets.Stamp
	}
	var pending []observation
	registry.mu.Lock()
	for id, definition := range registry.definitions {
		if include(id, definition) && !registry.removing[id] {
			pending = append(pending, observation{id: id, definition: definition, cached: registry.verified[id]})
		}
	}
	registry.mu.Unlock()
	for index := range pending {
		entry := &pending[index]
		path, err := directory(root, entry.id, entry.definition)
		if err != nil {
			return err
		}
		entry.stamp = observe(path, entry.definition, entry.cached)
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	for _, entry := range pending {
		registry.commit(entry.id, entry.definition, entry.cached, entry.stamp)
	}
	registry.publish()
	return nil
}

// Cached lists the models from verified stamps, without file access.
func (registry *Registry) Cached(kind protocol.ModelsKind) []protocol.ModelsLocalModel {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	models := []protocol.ModelsLocalModel{}
	for _, model := range registry.models() {
		if kind == "" || Kind(model.Definition.Format) == kind {
			models = append(models, model)
		}
	}
	return models
}

// models lists every definition in identity order. The caller holds mu.
func (registry *Registry) models() []protocol.ModelsLocalModel {
	models := make([]protocol.ModelsLocalModel, 0, len(registry.definitions))
	for _, id := range slices.Sorted(maps.Keys(registry.definitions)) {
		state := "missing"
		if registry.transfers[id] != nil {
			state = "downloading"
		} else if !registry.removing[id] && registry.verified[id] != nil {
			state = "installed"
		}
		models = append(models, protocol.ModelsLocalModel{ID: id, Definition: *registry.definitions[id], State: protocol.ModelsLocalState{Kind: state}})
	}
	return models
}

func (registry *Registry) update() (Update, string) {
	update := Update{
		Models:    registry.models(),
		Embedding: Choice(&registry.settings, protocol.ModelsKindEmbedding),
		OCR:       Choice(&registry.settings, protocol.ModelsKindOcr),
	}
	// Definitions are immutable per identity, so identities and states
	// determine the model list.
	var signature strings.Builder
	for _, model := range update.Models {
		signature.WriteString(model.ID + ":" + model.State.Kind + "\n")
	}
	choices, _ := json.Marshal([]protocol.ModelsLocalChoice{update.Embedding, update.OCR})
	signature.Write(choices)
	return update, signature.String()
}

// publish announces a changed state. The caller holds mu.
func (registry *Registry) publish() {
	update, signature := registry.update()
	if signature == registry.signature {
		return
	}
	registry.latest, registry.signature = update, signature
	close(registry.changed)
	registry.changed = make(chan struct{})
}

// Storage reports whether model storage is usable, without inspecting model
// files.
func (registry *Registry) Storage() error {
	_, err := registry.Root()
	return err
}

// Root is the model directory of the profile, created on first use.
func (registry *Registry) Root() (string, error) {
	registry.rootMu.Lock()
	defer registry.rootMu.Unlock()
	if registry.root != "" {
		return registry.root, nil
	}
	if registry.profile == "" {
		return "", newFault(protocol.ErrorCodeUnavailable, "local model storage is unavailable")
	}
	path, err := root(registry.profile)
	if err != nil {
		return "", err
	}
	registry.root = path
	return path, nil
}

// Definition is the registered definition of one identity.
func (registry *Registry) Definition(id string) (*protocol.ModelsDefinition, error) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if definition, registered := registry.definitions[id]; registered {
		return definition, nil
	}
	return nil, newFault(protocol.ErrorCodeNotFound, "local model is not registered")
}

// Resource captures a verified model. Executions check file identity once
// before running. A non-nil choice must still be the applied choice.
func (registry *Registry) Resource(kind protocol.ModelsKind, id string, choice *protocol.ModelsLocalChoice) (Resource, error) {
	var none Resource
	root, err := registry.Root()
	if err != nil {
		return none, err
	}
	registry.mu.Lock()
	if choice != nil && !sameChoice(Choice(&registry.settings, kind), *choice) {
		registry.mu.Unlock()
		return none, conflict()
	}
	definition, registered := registry.definitions[id]
	if !registered || Kind(definition.Format) != kind {
		registry.mu.Unlock()
		return none, invalid("local model kind does not match the request")
	}
	if registry.removing[id] {
		registry.mu.Unlock()
		return none, missing()
	}
	current, captured := registry.scopes[kind]
	cached := registry.verified[id]
	registry.mu.Unlock()
	if !captured {
		return none, speech()
	}
	path, err := directory(root, id, definition)
	if err != nil {
		return none, err
	}
	if cached == nil {
		// Uncached files are accepted through their receipt; hashing stays with listings.
		found := known(path, definition, nil)
		if found == nil {
			return none, missing()
		}
		registry.mu.Lock()
		switch {
		case registry.commit(id, definition, nil, found):
			registry.publish()
			cached = found
		case registry.removing[id] || registry.definitions[id] != definition:
			registry.mu.Unlock()
			return none, conflict()
		default:
			// A concurrent listing or installation recorded the current stamp first.
			cached = registry.verified[id]
		}
		registry.mu.Unlock()
		if cached == nil {
			return none, missing()
		}
	}
	return Resource{ID: id, Definition: definition, Directory: path, Stamp: cached, Scope: current.ctx}, nil
}

// current rejects a captured resource whose configuration, and optionally
// whose files, changed.
func (registry *Registry) current(resource Resource, inspect bool) error {
	if registry.stop.Err() != nil || resource.Scope.Err() != nil {
		return Cancelled()
	}
	registry.mu.Lock()
	stale := registry.removing[resource.ID] || registry.definitions[resource.ID] != resource.Definition || !resource.Stamp.Equal(registry.verified[resource.ID])
	registry.mu.Unlock()
	if stale {
		return conflict()
	}
	if inspect && !unchanged(resource) {
		registry.mu.Lock()
		if resource.Stamp.Equal(registry.verified[resource.ID]) {
			delete(registry.verified, resource.ID)
			registry.publish()
		}
		registry.mu.Unlock()
		return conflict()
	}
	return nil
}

// Execute runs one inference operation on a captured model. Interactive work
// runs before background work; a full lane reports Busy.
func Execute[T any](ctx context.Context, registry *Registry, resource Resource, lane Lane, operation func(ctx context.Context, engine any) (T, error)) (T, error) {
	check := func(inspect bool) error { return registry.current(resource, inspect) }
	value, err := registry.scheduler.request(ctx, resource, lane, check, false, func(ctx context.Context, engine any) (any, error) {
		return operation(ctx, engine)
	})
	if err != nil {
		var zero T
		return zero, err
	}
	return value.(T), nil
}

// Cancel stops the active installation of one model.
func (registry *Registry) Cancel(id string) bool {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	cancel := registry.transfers[id]
	if cancel != nil {
		cancel()
	}
	return cancel != nil
}

// Shutdown cancels every operation and releases every engine.
func (registry *Registry) Shutdown() {
	registry.cancel()
	registry.scheduler.shutdown()
	registry.tasksMu.Lock()
	registry.closed = true
	registry.tasksMu.Unlock()
	registry.tasks.Wait()
}

// commit applies an inspection unless installation, removal or another
// inspection changed the model first. The caller holds mu.
func (registry *Registry) commit(id string, definition *protocol.ModelsDefinition, cached, stamp *assets.Stamp) bool {
	if registry.removing[id] || registry.definitions[id] != definition || !cached.Equal(registry.verified[id]) {
		return false
	}
	if stamp == nil {
		delete(registry.verified, id)
	} else {
		registry.verified[id] = stamp
	}
	return true
}

func sourceChanged(definition *protocol.ModelsDefinition, previous, current *string) bool {
	if definition == nil || (previous == nil) == (current == nil) && (previous == nil || *previous == *current) {
		return false
	}
	for _, asset := range definition.Assets {
		before, beforeErr := assets.URL(asset, previous)
		after, afterErr := assets.URL(asset, current)
		if (beforeErr == nil) != (afterErr == nil) || beforeErr == nil && before.String() != after.String() {
			return true
		}
	}
	return false
}
