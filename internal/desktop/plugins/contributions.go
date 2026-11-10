package plugins

import (
	"context"
	"encoding/json"
	"math"
	"reflect"
	"slices"
	"strings"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const contributionQueue = 16

var contributionGenerations struct {
	sync.Mutex
	value uint64
}

type RegistryOptions struct {
	Node       protocol.NodeID
	Context    protocol.PluginContext
	Invalidate func()
}

type ContributionKey struct {
	Node       protocol.NodeID          `json:"node"`
	Worktree   *protocol.WorktreeID     `json:"worktree"`
	Session    *protocol.SessionID      `json:"session"`
	Package    protocol.PluginReference `json:"package"`
	ID         string                   `json:"id"`
	Generation uint64                   `json:"generation"`
}

type ContributionEntry struct {
	Key         ContributionKey               `json:"key"`
	Declaration protocol.PluginUiContribution `json:"declaration"`
	State       protocol.PluginUiState        `json:"state"`
	Retry       bool                          `json:"retry"`
}

type Registry struct {
	mu         sync.Mutex
	node       protocol.NodeID
	scope      protocol.PluginContext
	invalidate func()
	generation uint64
	mounted    map[string]*contributionMount
	closed     bool
}

type contributionMount struct {
	reference    protocol.PluginReference
	generation   uint64
	declarations []protocol.PluginUiContribution
	states       map[string]protocol.PluginUiState
	published    map[string]bool
	retained     map[string]bool
	scopes       []protocol.PluginUiProviderScope
	interactions map[string]contributionInteraction
	closed       map[string]bool
	sequence     uint64
	active       bool
	failed       bool
	actions      chan protocol.PluginUiEvent
	receiver     chan struct{}
	done         chan struct{}
}

type contributionInteraction struct {
	sequence uint64
	busy     bool
}

func NewRegistry(options RegistryOptions) *Registry {
	return &Registry{node: options.Node, scope: contributionCopy(options.Context), invalidate: options.Invalidate, mounted: map[string]*contributionMount{}}
}

func (registry *Registry) Hooks(reference protocol.PluginReference, declarations []protocol.PluginUiContribution) (ContributionHooks, error) {
	body, err := json.Marshal(declarations)
	if err != nil {
		return ContributionHooks{}, contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution declarations")
	}
	var captured []protocol.PluginUiContribution
	if err := json.Unmarshal(body, &captured); err != nil || !protocol.ValidPluginUiContributions(captured) {
		return ContributionHooks{}, contributionFault(protocol.ErrorCodeInvalidRequest, "invalid contribution declarations")
	}
	registry.mu.Lock()
	if registry.closed {
		registry.mu.Unlock()
		return ContributionHooks{}, contributionFault(protocol.ErrorCodeUnavailable, "contribution registry is unavailable")
	}
	contributionGenerations.Lock()
	if contributionGenerations.value == math.MaxUint64 {
		contributionGenerations.Unlock()
		registry.mu.Unlock()
		return ContributionHooks{}, contributionFault(protocol.ErrorCodeUnavailable, "contribution registry is unavailable")
	}
	contributionGenerations.value++
	registry.generation = contributionGenerations.value
	contributionGenerations.Unlock()
	mount := &contributionMount{
		reference: reference, generation: registry.generation, declarations: captured,
		states: map[string]protocol.PluginUiState{}, published: map[string]bool{}, retained: map[string]bool{},
		interactions: map[string]contributionInteraction{}, closed: map[string]bool{},
		actions: make(chan protocol.PluginUiEvent, contributionQueue), receiver: make(chan struct{}, 1), done: make(chan struct{}),
	}
	for _, declaration := range captured {
		if declaration.Slot != "status" {
			state := protocol.DefaultPluginUiState()
			state.ID = declaration.ID
			mount.states[declaration.ID] = state
		}
	}
	if previous := registry.mounted[reference.Name]; previous != nil {
		if previous.reference == reference {
			mount.sequence = previous.sequence
			mount.scopes = slices.Clone(previous.scopes)
			for key, state := range previous.states {
				state.Enabled, state.ReplyTo = false, nil
				mount.states[key] = contributionCopy(state)
			}
			for key := range previous.retained {
				mount.retained[key] = true
			}
		}
		close(previous.done)
	}
	registry.mounted[reference.Name] = mount
	registry.mu.Unlock()
	registry.changed()
	return ContributionHooks{
		Publish: func(body json.RawMessage) error { return registry.publish(mount, body) },
		Close:   func(id string) error { return registry.closePopover(mount, id) },
		Next:    func(ctx context.Context) (json.RawMessage, error) { return registry.next(ctx, mount) },
	}, nil
}

func (registry *Registry) SetActivity(reference protocol.PluginReference, active, failed bool) error {
	registry.mu.Lock()
	mount, err := registry.current(reference)
	if err == nil {
		mount.active, mount.failed = active, failed
	}
	registry.mu.Unlock()
	if err == nil {
		registry.changed()
	}
	return err
}

func (registry *Registry) SetProviderScopes(reference protocol.PluginReference, scopes []protocol.PluginUiProviderScope) error {
	for _, scope := range scopes {
		if !scope.Valid() {
			return contributionFault(protocol.ErrorCodeInvalidRequest, "invalid provider contribution scope")
		}
	}
	registry.mu.Lock()
	mount, err := registry.current(reference)
	if err == nil {
		for key, state := range mount.states {
			if state.Scope != nil && !slices.Contains(scopes, *state.Scope) {
				delete(mount.states, key)
			}
		}
		mount.scopes = slices.Clone(scopes)
	}
	registry.mu.Unlock()
	if err == nil {
		registry.changed()
	}
	return err
}

func (registry *Registry) Ready(reference protocol.PluginReference) bool {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	mount, err := registry.current(reference)
	return err == nil && len(mount.published) != 0
}

func (registry *Registry) Key(reference protocol.PluginReference, id string) (ContributionKey, bool) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	mount, err := registry.current(reference)
	if err != nil || !slices.ContainsFunc(mount.declarations, func(declaration protocol.PluginUiContribution) bool { return declaration.ID == id }) {
		return ContributionKey{}, false
	}
	return contributionCopy(registry.key(mount, id)), true
}

func (registry *Registry) Entries(slot protocol.PluginUiSlot) []ContributionEntry {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	entries := []ContributionEntry{}
	if registry.closed || slot == "status" || !registry.accepts(slot) {
		return entries
	}
	for _, mount := range registry.mounted {
		for _, declaration := range mount.declarations {
			if declaration.Slot != slot {
				continue
			}
			state, exists := mount.states[declaration.ID]
			if !exists {
				continue
			}
			if !mount.retained[declaration.ID] {
				state.Visible, state.Enabled = mount.failed || declaration.Kind == "indicator", false
			}
			if !mount.published[declaration.ID] {
				state.Loading, state.Enabled = !mount.failed, false
			}
			state.Enabled = state.Enabled && mount.active && !state.Loading
			if state.Visible {
				entries = append(entries, contributionCopy(ContributionEntry{
					Key: registry.key(mount, declaration.ID), Declaration: declaration, State: state, Retry: mount.failed,
				}))
			}
		}
	}
	sortContributions(entries)
	return entries
}

func (registry *Registry) Commands() []ContributionEntry {
	entries, counts := []ContributionEntry{}, map[string]int{}
	for _, slot := range []protocol.PluginUiSlot{"composer", "context", "commands"} {
		for _, entry := range registry.Entries(slot) {
			if entry.Declaration.Command != nil {
				entries = append(entries, entry)
				counts[entry.Declaration.Command.Name]++
			}
		}
	}
	for index := range entries {
		entries[index].State.Enabled = entries[index].State.Enabled && counts[entries[index].Declaration.Command.Name] == 1
	}
	sortContributions(entries)
	return entries
}

func (registry *Registry) Intent(intent string) (ContributionEntry, bool) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if registry.closed {
		return ContributionEntry{}, false
	}
	names := make([]string, 0, len(registry.mounted))
	for name := range registry.mounted {
		names = append(names, name)
	}
	slices.Sort(names)
	for _, name := range names {
		mount := registry.mounted[name]
		if !mount.active {
			continue
		}
		var selected *protocol.PluginUiContribution
		for index := range mount.declarations {
			declaration := &mount.declarations[index]
			if declaration.Slot == "status" || declaration.Intent == nil || *declaration.Intent != intent {
				continue
			}
			if selected == nil || registry.accepts(declaration.Slot) && !registry.accepts(selected.Slot) {
				selected = declaration
			}
		}
		if selected == nil {
			continue
		}
		state, exists := mount.states[selected.ID]
		if exists && mount.published[selected.ID] && state.Enabled && !state.Loading {
			return contributionCopy(ContributionEntry{Key: registry.key(mount, selected.ID), Declaration: *selected, State: state}), true
		}
	}
	return ContributionEntry{}, false
}

func (registry *Registry) IntentReady(intent string) (bool, bool) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if registry.closed {
		return false, false
	}
	declared, ready := false, false
	for _, mount := range registry.mounted {
		var selected *protocol.PluginUiContribution
		for index := range mount.declarations {
			declaration := &mount.declarations[index]
			if declaration.Slot == "status" || declaration.Intent == nil || *declaration.Intent != intent {
				continue
			}
			if selected == nil || registry.accepts(declaration.Slot) && !registry.accepts(selected.Slot) {
				selected = declaration
			}
		}
		if selected != nil {
			declared = true
			ready = ready || mount.failed || mount.active && mount.published[selected.ID]
		}
	}
	return declared, ready
}

func (registry *Registry) ProviderEntries(scope protocol.PluginUiProviderScope) []ContributionEntry {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	entries := []ContributionEntry{}
	if registry.closed || scope.Credential.Node != registry.node {
		return entries
	}
	for _, mount := range registry.mounted {
		if !mount.active {
			continue
		}
		for _, state := range mount.states {
			if !state.Visible || !state.Enabled || state.Scope == nil || *state.Scope != scope {
				continue
			}
			for _, declaration := range mount.declarations {
				if declaration.ID == state.ID && declaration.Slot == "provider" {
					key := registry.key(mount, state.ID)
					key.Worktree, key.Session = nil, nil
					entries = append(entries, contributionCopy(ContributionEntry{Key: key, Declaration: declaration, State: state}))
				}
			}
		}
	}
	sortContributions(entries)
	return entries
}

func (registry *Registry) Invoke(key ContributionKey, kind protocol.PluginUiEventKind, value json.RawMessage) error {
	return registry.dispatch(key, kind, value, false, false)
}

func (registry *Registry) InvokeIntent(key ContributionKey, value json.RawMessage) error {
	return registry.dispatch(key, "invoke", value, true, false)
}

func (registry *Registry) ForwardStatus(key ContributionKey, kind protocol.PluginUiEventKind, value json.RawMessage) error {
	return registry.dispatch(key, kind, value, false, true)
}

func (registry *Registry) TakeClosed(reference protocol.PluginReference) []string {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	result := []string{}
	mount, err := registry.current(reference)
	if err != nil {
		return result
	}
	for id := range mount.closed {
		result = append(result, id)
	}
	clear(mount.closed)
	slices.Sort(result)
	return result
}

func (registry *Registry) Close() error {
	registry.mu.Lock()
	if registry.closed {
		registry.mu.Unlock()
		return nil
	}
	registry.closed = true
	for _, mount := range registry.mounted {
		close(mount.done)
	}
	clear(registry.mounted)
	registry.mu.Unlock()
	registry.changed()
	return nil
}

func (registry *Registry) current(reference protocol.PluginReference) (*contributionMount, error) {
	mount := registry.mounted[reference.Name]
	if registry.closed || mount == nil || mount.reference != reference {
		return nil, contributionFault(protocol.ErrorCodeRevisionConflict, "contribution context changed")
	}
	return mount, nil
}

func (registry *Registry) owns(mount *contributionMount) bool {
	return !registry.closed && registry.mounted[mount.reference.Name] == mount
}

func (registry *Registry) key(mount *contributionMount, id string) ContributionKey {
	return ContributionKey{Node: registry.node, Worktree: registry.scope.Worktree, Session: registry.scope.Session, Package: mount.reference, ID: id, Generation: mount.generation}
}

func (registry *Registry) accepts(slot protocol.PluginUiSlot) bool {
	switch registry.scope.Surface.Kind {
	case "provider":
		return slot == "provider"
	case "project":
		return slot == "project" || slot == "project_menu"
	default:
		return slot != "project" && slot != "project_menu" && slot != "provider"
	}
}

func (registry *Registry) matches(key ContributionKey, mount *contributionMount) bool {
	return registry.owns(mount) && key.Node == registry.node && key.Package == mount.reference && key.Generation == mount.generation &&
		reflect.DeepEqual(key.Worktree, registry.scope.Worktree) && reflect.DeepEqual(key.Session, registry.scope.Session)
}

func (registry *Registry) changed() {
	if registry.invalidate != nil {
		registry.invalidate()
	}
}

func contributionKey(state protocol.PluginUiState) string {
	if state.Scope == nil {
		return state.ID
	}
	return state.ID + ":" + state.Scope.Provider.String()
}

func sortContributions(entries []ContributionEntry) {
	slices.SortFunc(entries, func(first, second ContributionEntry) int {
		if first.Declaration.Order < second.Declaration.Order {
			return -1
		}
		if first.Declaration.Order > second.Declaration.Order {
			return 1
		}
		if order := strings.Compare(first.Key.Package.Name, second.Key.Package.Name); order != 0 {
			return order
		}
		return strings.Compare(first.Key.ID, second.Key.ID)
	})
}

func contributionFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}

func contributionCopy[Value any](value Value) Value {
	body, _ := json.Marshal(value)
	var result Value
	_ = json.Unmarshal(body, &result)
	return result
}
