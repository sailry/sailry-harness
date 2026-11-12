package plugins

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type NativeBinding struct {
	Context            context.Context
	Post               func(func(*ui.Context)) bool
	Invalidate         func()
	Frame              func(*ui.Context)
	Release            func()
	PreparedRequest    func(context.Context, string) (protocol.Request, error)
	Theme              func() json.RawMessage
	TerminalAppearance func() (protocol.TerminalAppearance, error)
	Operations         map[string]HostOperation
	Components         []NativeComponent
	ChooseSecretFile   func(context.Context, string) ([]byte, string, error)
	Header             func(json.RawMessage) error
	// Models are the Node's models as the application names them: a
	// ModelPopup shows the names, marks and scores of the ones its catalog
	// lists. Without it a model shows under its catalog's own identifiers.
	Models func() []kit.ModelOption
	state  *nativeState
}

type nativeState struct {
	ctx          context.Context
	cancel       context.CancelFunc
	mu           sync.Mutex
	sequence     uint64
	text         map[string]*textDraft
	dates        map[string]*dateDraft
	sliders      map[string]*sliderDraft
	secrets      map[string]*secretDraft
	queues       map[string]*nativeQueue
	tables       map[string]*tableDraft
	trees        map[string]*treeDraft
	modals       map[string]*modalDraft
	pickers      map[string]*pickerDraft
	measure      map[string]uint32
	loadMore     map[string]bool
	droppedFiles map[string][]string
	hoverGroups  map[string]bool
	focus        string
	// rowLead is set while a group whose rows have icons draws its rows;
	// only the frame's own goroutine touches it.
	rowLead         bool
	shortcut        string
	shortcuts       []nativeShortcut
	shortcutVersion uint64
	actionFrame     time.Time
	actionElements  map[uint64]ui.Element
	booleans        map[string]*bool
	strings         map[string]*string
	collapses       map[string]*bool
	header          json.RawMessage
	failure         error
	closed          bool
	suspended       bool
	workers         sync.WaitGroup
}

type nativeQueue struct {
	events  chan json.RawMessage
	pending chan struct{}
}

func NewNativeBinding(ctx context.Context) NativeBinding {
	value := NativeBinding{Context: ctx}
	value.ensure(ctx)
	return value
}

func (binding *NativeBinding) ensure(ctx context.Context) {
	if binding.state != nil {
		return
	}
	if ctx == nil {
		ctx = context.Background()
	}
	if binding.Context != nil {
		ctx = binding.Context
	}
	lifetime, cancel := context.WithCancel(ctx)
	binding.Context = lifetime
	binding.state = &nativeState{
		ctx: lifetime, cancel: cancel,
		text: map[string]*textDraft{}, dates: map[string]*dateDraft{},
		sliders: map[string]*sliderDraft{}, secrets: map[string]*secretDraft{},
		queues: map[string]*nativeQueue{}, tables: map[string]*tableDraft{},
		trees: map[string]*treeDraft{}, modals: map[string]*modalDraft{}, pickers: map[string]*pickerDraft{},
		measure: map[string]uint32{}, loadMore: map[string]bool{},
		booleans: map[string]*bool{}, strings: map[string]*string{}, collapses: map[string]*bool{},
		droppedFiles: map[string][]string{}, hoverGroups: map[string]bool{},
		actionElements: map[uint64]ui.Element{},
	}
}

func (binding *NativeBinding) close() {
	binding.state.mu.Lock()
	if binding.state.closed {
		binding.state.mu.Unlock()
		return
	}
	binding.state.closed = true
	binding.state.suspended = false
	binding.state.cancel()
	for _, secret := range binding.state.secrets {
		secret.value = ""
	}
	binding.state.text = map[string]*textDraft{}
	binding.state.secrets = map[string]*secretDraft{}
	clear(binding.state.droppedFiles)
	clear(binding.state.actionElements)
	binding.state.mu.Unlock()
	binding.state.workers.Wait()
	if binding.Release != nil {
		binding.Release()
	}
}

func (binding *NativeBinding) suspend() {
	binding.state.mu.Lock()
	if binding.state.closed || binding.state.suspended {
		binding.state.mu.Unlock()
		return
	}
	binding.state.suspended = true
	binding.state.cancel()
	for _, secret := range binding.state.secrets {
		secret.value = ""
	}
	clear(binding.state.droppedFiles)
	clear(binding.state.actionElements)
	binding.state.mu.Unlock()
	binding.state.workers.Wait()
}

func (binding *NativeBinding) Suspended() bool {
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	return binding.state.suspended && !binding.state.closed
}

func (binding *NativeBinding) queue(name string, capacity int) *nativeQueue {
	switch name {
	case "tree":
		capacity = 128
	case "table":
		capacity = 64
	}
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	queue, exists := binding.state.queues[name]
	if !exists {
		queue = &nativeQueue{events: make(chan json.RawMessage, capacity), pending: make(chan struct{}, 1)}
		binding.state.queues[name] = queue
	}
	return queue
}

func (binding *NativeBinding) emit(name string, value any) {
	body, err := json.Marshal(value)
	if err != nil {
		return
	}
	queue := binding.queue(name, 32)
	select {
	case <-binding.state.ctx.Done():
	case queue.events <- json.RawMessage(body):
	default:
	}
	if binding.Invalidate != nil {
		binding.Invalidate()
	}
}

func (binding *NativeBinding) emitGuaranteed(name string, value any) {
	body, err := json.Marshal(value)
	if err != nil {
		binding.fail(err)
		return
	}
	queue := binding.queue(name, 32)
	binding.state.mu.Lock()
	if binding.state.closed {
		binding.state.mu.Unlock()
		return
	}
	binding.state.workers.Add(1)
	binding.state.mu.Unlock()
	go func() {
		defer binding.state.workers.Done()
		select {
		case <-binding.state.ctx.Done():
			return
		case queue.events <- json.RawMessage(body):
		}
		if binding.Invalidate != nil {
			binding.Invalidate()
		}
	}()
}

func (binding *NativeBinding) next(ctx context.Context, name string) (json.RawMessage, error) {
	queue := binding.queue(name, 32)
	select {
	case queue.pending <- struct{}{}:
		defer func() { <-queue.pending }()
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-binding.state.ctx.Done():
		return nil, binding.state.ctx.Err()
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-binding.state.ctx.Done():
		return nil, binding.state.ctx.Err()
	case event := <-queue.events:
		return event, nil
	}
}

func (binding *NativeBinding) operation(ctx context.Context, module, name string, arguments []json.RawMessage) (json.RawMessage, error) {
	if err := binding.state.ctx.Err(); err != nil {
		return nil, err
	}
	switch module {
	case "sailry/forms":
		return binding.formOperation(ctx, name, arguments)
	case "sailry/credentials":
		return binding.credentialOperation(ctx, name, arguments)
	case "sailry/ui":
		if handled, value, err := binding.uiOperation(ctx, name, arguments); handled {
			return value, err
		}
	case "sailry":
		if name == "theme" && binding.Theme != nil {
			return binding.Theme(), nil
		}
		if name == "theme" {
			return ThemeSnapshot(nil), nil
		}
		if name == "header_action" {
			return binding.next(ctx, "header")
		}
	}
	if operation := binding.Operations[module+"."+name]; operation != nil {
		return operation(ctx, arguments)
	}
	return nil, hostUnavailable(fmt.Sprintf("%s.%s is unavailable in this view", module, name))
}

func nativeProps(element NativeElement) (string, map[string]json.RawMessage, error) {
	identity := fmt.Sprintf("native-%d", element.ID)
	props := map[string]json.RawMessage{}
	if len(element.Props) == 0 || string(element.Props) == "null" {
		return identity, props, nil
	}
	var positional []json.RawMessage
	if json.Unmarshal(element.Props, &positional) == nil {
		if len(positional) > 0 {
			if err := json.Unmarshal(positional[0], &identity); err != nil {
				return "", nil, hostInvalid("native component requires an identifier")
			}
		}
		if len(positional) > 1 && string(positional[1]) != "null" {
			if json.Unmarshal(positional[1], &props) != nil {
				return "", nil, hostInvalid("native component props must be an object")
			}
		}
		return identity, props, nil
	}
	if json.Unmarshal(element.Props, &props) != nil {
		return "", nil, hostInvalid("native component props must be an object")
	}
	if value, exists := props["id"]; exists {
		if json.Unmarshal(value, &identity) != nil {
			return "", nil, hostInvalid("native component requires an identifier")
		}
	} else if value, exists := props["handle"]; exists {
		if json.Unmarshal(value, &identity) != nil {
			return "", nil, hostInvalid("native component requires an identifier")
		}
	}
	return identity, props, nil
}

func nativeProp[Value any](props map[string]json.RawMessage, key string, fallback Value) Value {
	body, exists := props[key]
	if !exists {
		return fallback
	}
	var value Value
	if json.Unmarshal(body, &value) != nil {
		return fallback
	}
	return value
}

func renderNativeChildren(view *ui.Context, element NativeElement, children []*NativeElement) error {
	if len(children) == 0 {
		return nil
	}
	if element.RenderChild == nil {
		return hostUnavailable("selective native child rendering is unavailable")
	}
	for _, child := range children {
		if _, err := element.RenderChild(view, child); err != nil {
			return err
		}
	}
	return nil
}

func NativeComponents(binding NativeBinding) ([]NativeComponent, error) {
	binding.ensure(binding.Context)
	components := binding.components()
	for _, supplied := range binding.Components {
		replaced := false
		for index, existing := range components {
			if existing.Module == supplied.Module && existing.Export == supplied.Export {
				components[index] = supplied
				replaced = true
				break
			}
		}
		if !replaced {
			components = append(components, supplied)
		}
	}
	return components, nil
}
