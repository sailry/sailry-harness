package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type HostBinding struct {
	Context       context.Context
	Client        *client.Client
	Target        protocol.NodeID
	Scope         protocol.PluginContext
	Locale        string
	Contributions ContributionHooks
	Native        *NativeBinding
	Clock         clockwork.Clock
	ReadFiles     bool
	Actions       []protocol.PluginAction
	UsageSources  func() []UsageSource
	UsageChanges  <-chan struct{}
}

type UsageSource struct {
	Client *client.Client
	Label  string
}

type Host struct {
	binding             HostBinding
	ctx                 context.Context
	cancel              context.CancelFunc
	mu                  sync.Mutex
	closed              bool
	requests            map[string]protocol.Request
	observers           map[string]*client.Observer
	feeds               map[string]*hostFeed
	topics              map[string]link.Subscription
	native              *NativeBinding
	changes             *hostChanges
	settings            hostSettings
	installation        *hostInstallationScope
	installationPending chan struct{}
	fileSearchCancel    context.CancelFunc
	usageChanged        chan struct{}
	usageSourcesClosed  bool
}

func NewHost(binding HostBinding) *Host {
	if binding.Context == nil {
		binding.Context = context.Background()
	}
	ctx, cancel := context.WithCancel(binding.Context)
	if binding.Client != nil {
		binding.Target = binding.Client.Target()
	}
	if binding.Locale == "" {
		binding.Locale = "en"
	}
	if binding.Clock == nil {
		binding.Clock = clockwork.NewRealClock()
	}
	if binding.Native == nil {
		value := NewNativeBinding(ctx)
		binding.Native = &value
	}
	binding.Native.ensure(ctx)
	owner := &Host{
		binding: binding, ctx: ctx, cancel: cancel, native: binding.Native,
		requests: map[string]protocol.Request{}, observers: map[string]*client.Observer{}, feeds: map[string]*hostFeed{},
		topics:              map[string]link.Subscription{},
		installationPending: make(chan struct{}, 1),
		usageChanged:        make(chan struct{}),
	}
	owner.settings.current = binding.Scope.Package
	owner.native.PreparedRequest = owner.prepared
	owner.settings.surface = binding.Scope.Surface.Kind == "settings"
	owner.changes = newHostChanges(owner)
	owner.changes.start()
	if binding.UsageChanges != nil {
		go owner.observeUsageSources()
	}
	return owner
}

func (owner *Host) Native() NativeBinding {
	return *owner.native
}

func (owner *Host) Close() error {
	return owner.disconnect(true)
}

func (owner *Host) Disconnect() error {
	return owner.disconnect(false)
}

func (owner *Host) disconnect(release bool) error {
	owner.mu.Lock()
	if owner.closed {
		owner.mu.Unlock()
		if release {
			owner.native.close()
		}
		return nil
	}
	owner.closed = true
	observers := owner.observers
	topics := owner.topics
	owner.observers = map[string]*client.Observer{}
	owner.topics = map[string]link.Subscription{}
	owner.requests = map[string]protocol.Request{}
	owner.mu.Unlock()
	if !release {
		owner.native.suspend()
	}
	owner.cancel()
	owner.changes.close()
	var failures []error
	for _, observer := range observers {
		failures = append(failures, observer.Close())
	}
	for _, topic := range topics {
		failures = append(failures, topic.Close())
	}
	if release {
		owner.native.close()
	}
	return errors.Join(failures...)
}

func (owner *Host) check(ctx context.Context) error {
	if err := owner.ctx.Err(); err != nil {
		return err
	}
	return ctx.Err()
}

func (owner *Host) target() protocol.NodeID {
	return owner.binding.Target
}

func (owner *Host) scope() protocol.PluginContext {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	scope := owner.binding.Scope
	scope.Package = owner.settings.current
	return scope
}

func (owner *Host) nativeOperation(module, name string) HostOperation {
	return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if err := owner.check(ctx); err != nil {
			return nil, err
		}
		return owner.native.operation(ctx, module, name, arguments)
	}
}

func hostInvalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func hostUnavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func hostFault(err error) protocol.Fault {
	var wrapped *link.FaultError
	if errors.As(err, &wrapped) {
		return wrapped.Fault
	}
	var pointer *protocol.Fault
	if errors.As(err, &pointer) {
		return *pointer
	}
	var direct protocol.Fault
	if errors.As(err, &direct) {
		return direct
	}
	if errors.Is(err, context.Canceled) {
		return protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "plugin view is closed"}
	}
	return protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: err.Error()}
}

func hostJSON(value any) (json.RawMessage, error) {
	body, err := json.Marshal(value)
	return json.RawMessage(body), err
}

func hostValue[Value any](arguments []json.RawMessage, index int) (Value, error) {
	var value Value
	if index >= len(arguments) || json.Unmarshal(arguments[index], &value) != nil {
		return value, hostInvalid("invalid plugin host argument")
	}
	return value, nil
}

func hostOptional[Value any](arguments []json.RawMessage, index int, fallback Value) (Value, error) {
	if index >= len(arguments) || string(arguments[index]) == "null" {
		return fallback, nil
	}
	return hostValue[Value](arguments, index)
}

func hostSync(name string, operation HostOperation) HostExport {
	return HostExport{Name: name, Sync: operation}
}

func hostAsync(name string, operation HostOperation) HostExport {
	return HostExport{Name: name, Async: operation}
}
