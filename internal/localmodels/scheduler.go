package localmodels

import (
	"context"
	"sync"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// idle is how long an unused engine stays loaded.
	idle = 300 * time.Second
	// memoryBytes bounds the summed asset sizes of loaded engines.
	memoryBytes uint64 = 2 * 1024 * 1024 * 1024
	// Queued work per lane, beside the running operation.
	interactiveCapacity = 8
	backgroundCapacity  = 4
)

// Lane orders queued inference. Interactive work always runs before
// background work.
type Lane uint8

const (
	Interactive Lane = iota
	Background
)

// Resource is a verified model captured for one operation.
type Resource struct {
	ID         string
	Definition *protocol.ModelsDefinition
	Directory  string
	Stamp      *assets.Stamp
	// Scope ends when the model's kind is reconfigured or the Node stops.
	Scope context.Context
}

// Engine is a loaded model. The scheduler owns it and calls Release once
// when it unloads the model.
type Engine struct {
	// Value is the runtime's own engine, passed back to its operations.
	Value   any
	Release func()
}

// Load builds the engine of one verified model on the scheduler's worker.
// validating is true while an installation checks compatibility.
type Load func(resource Resource, validating bool) (Engine, error)

// Capability is the inference runtime of one model kind.
type Capability struct {
	Load Load
	// Available reports whether this machine can run the kind. Nil means it can.
	Available func() error
	// Probe is the compatibility check an installation runs on the freshly
	// loaded engine. Nil means loading is the whole check.
	Probe func(ctx context.Context, engine any) error
}

// Capabilities are the runtimes present in this build. A kind without one is
// unavailable.
type Capabilities map[protocol.ModelsKind]Capability

// Ensure reports why a kind cannot execute on this Node.
func (capabilities Capabilities) Ensure(kind protocol.ModelsKind) error {
	if kind == protocol.ModelsKindSpeech {
		return speech()
	}
	capability, present := capabilities[kind]
	if !present || capability.Load == nil {
		return newFault(protocol.ErrorCodeUnavailable, "local "+string(kind)+" runtime is unavailable")
	}
	if capability.Available != nil {
		if err := capability.Available(); err != nil {
			return newFault(protocol.ErrorCodeUnavailable, err.Error())
		}
	}
	return nil
}

// Catalog pairs models with the execution availability of every Node kind.
func (capabilities Capabilities) Catalog(models []protocol.ModelsLocalModel) protocol.ModelsCatalog {
	catalog := protocol.ModelsCatalog{Models: models, Availability: map[protocol.ModelsKind]protocol.ModelsCatalogAvailability{}}
	for _, kind := range NodeKinds {
		status := protocol.ModelsCatalogAvailability{Kind: "available"}
		if err := capabilities.Ensure(kind); err != nil {
			status = protocol.ModelsCatalogAvailability{Kind: "unavailable", Data: protocol.ModelsCatalogAvailabilityUnavailablePayload{Reason: err.(*protocol.Fault).Message}}
		}
		catalog.Availability[kind] = status
	}
	return catalog
}

// work is one queued operation. call receives the engine or the reason none
// was acquired and reports whether the operation succeeded.
type work struct {
	resource   Resource
	ctx        context.Context
	check      func(files bool) error
	call       func(engine *Engine, err error) bool
	validating bool
}

type loaded struct {
	resource Resource
	used     time.Time
	bytes    uint64
	engine   Engine
}

// scheduler runs every local inference on one worker: at most one operation
// at a time, interactive before background, with idle engines released.
type scheduler struct {
	clock        clockwork.Clock
	capabilities Capabilities
	stop         context.Context
	cancel       context.CancelFunc
	wake         chan struct{}
	done         chan struct{}

	mu          sync.Mutex
	interactive []*work
	background  []*work
	unloads     []unload
}

type unload struct {
	id    string
	reply chan struct{}
}

func newScheduler(stop context.Context, clock clockwork.Clock, capabilities Capabilities) *scheduler {
	owned, cancel := context.WithCancel(stop)
	scheduler := &scheduler{clock: clock, capabilities: capabilities, stop: owned, cancel: cancel, wake: make(chan struct{}, 1), done: make(chan struct{})}
	go scheduler.run()
	return scheduler
}

func (scheduler *scheduler) signal() {
	select {
	case scheduler.wake <- struct{}{}:
	default:
	}
}

// request queues an operation and waits for its result. Leaving early cancels
// the queued or running operation.
func (scheduler *scheduler) request(ctx context.Context, resource Resource, lane Lane, check func(bool) error, validating bool, operation func(context.Context, any) (any, error)) (any, error) {
	if ctx.Err() != nil || scheduler.stop.Err() != nil || resource.Scope.Err() != nil {
		return nil, Cancelled()
	}
	scoped, cancel := context.WithCancel(scheduler.stop)
	defer cancel()
	type outcome struct {
		value any
		err   error
	}
	reply := make(chan outcome, 1)
	item := &work{resource: resource, ctx: scoped, check: check, validating: validating, call: func(engine *Engine, err error) bool {
		var value any
		if err == nil {
			value, err = operation(scoped, engine.Value)
		}
		if scoped.Err() != nil {
			err = Cancelled()
		} else if current := check(false); current != nil {
			err = current
		}
		reply <- outcome{value, err}
		return err == nil
	}}
	scheduler.mu.Lock()
	queue, capacity := &scheduler.interactive, interactiveCapacity
	if lane == Background {
		queue, capacity = &scheduler.background, backgroundCapacity
	}
	if len(*queue) >= capacity {
		scheduler.mu.Unlock()
		return nil, newFault(protocol.ErrorCodeBusy, "local inference queue is full")
	}
	*queue = append(*queue, item)
	scheduler.mu.Unlock()
	scheduler.signal()
	select {
	case <-ctx.Done():
	case <-scheduler.stop.Done():
	case <-resource.Scope.Done():
	case result := <-reply:
		return result.value, result.err
	}
	return nil, Cancelled()
}

// unload releases one engine and waits until the worker has dropped it.
func (scheduler *scheduler) unload(id string) error {
	reply := make(chan struct{})
	scheduler.mu.Lock()
	scheduler.unloads = append(scheduler.unloads, unload{id: id, reply: reply})
	scheduler.mu.Unlock()
	scheduler.signal()
	select {
	case <-reply:
		return nil
	case <-scheduler.done:
		return Cancelled()
	}
}

func (scheduler *scheduler) shutdown() {
	scheduler.cancel()
	<-scheduler.done
}

// next takes the most urgent queued item: unloads, then interactive work,
// then background work.
func (scheduler *scheduler) next() (*unload, *work) {
	scheduler.mu.Lock()
	defer scheduler.mu.Unlock()
	if len(scheduler.unloads) > 0 {
		request := scheduler.unloads[0]
		scheduler.unloads = scheduler.unloads[1:]
		return &request, nil
	}
	for _, queue := range []*[]*work{&scheduler.interactive, &scheduler.background} {
		if len(*queue) > 0 {
			item := (*queue)[0]
			*queue = (*queue)[1:]
			return nil, item
		}
	}
	return nil, nil
}

func (scheduler *scheduler) run() {
	defer close(scheduler.done)
	engines := map[string]*loaded{}
	release := func(id string) {
		if entry, present := engines[id]; present {
			delete(engines, id)
			if entry.engine.Release != nil {
				entry.engine.Release()
			}
		}
	}
	for scheduler.stop.Err() == nil {
		request, item := scheduler.next()
		switch {
		case request != nil:
			release(request.id)
			close(request.reply)
		case item != nil:
			scheduler.execute(item, engines, release)
		default:
			deadline := scheduler.clock.Now().Add(idle)
			for _, entry := range engines {
				if expiry := entry.used.Add(idle); expiry.Before(deadline) {
					deadline = expiry
				}
			}
			timer := scheduler.clock.NewTimer(deadline.Sub(scheduler.clock.Now()))
			select {
			case <-scheduler.stop.Done():
			case <-scheduler.wake:
			case <-timer.Chan():
				now := scheduler.clock.Now()
				for id, entry := range engines {
					if !entry.used.Add(idle).After(now) {
						release(id)
					}
				}
			}
			timer.Stop()
		}
	}
	for id := range engines {
		release(id)
	}
	scheduler.mu.Lock()
	pending := append(scheduler.interactive, scheduler.background...)
	scheduler.interactive, scheduler.background = nil, nil
	scheduler.mu.Unlock()
	for _, item := range pending {
		item.call(nil, Cancelled())
	}
}

func (scheduler *scheduler) execute(item *work, engines map[string]*loaded, release func(string)) {
	entry, err := scheduler.acquire(item, engines, release)
	var succeeded bool
	if err != nil {
		succeeded = item.call(nil, err)
	} else {
		succeeded = item.call(&entry.engine, nil)
		entry.used = scheduler.clock.Now()
	}
	if item.validating && !succeeded {
		release(item.resource.ID)
	}
}

func (scheduler *scheduler) acquire(item *work, engines map[string]*loaded, release func(string)) (*loaded, error) {
	if item.ctx.Err() != nil || scheduler.stop.Err() != nil {
		return nil, Cancelled()
	}
	if err := item.check(!item.validating); err != nil {
		return nil, err
	}
	resource := item.resource
	if item.validating && !resource.Stamp.Equal(assets.Candidate(resource.Directory, files(resource.Definition))) {
		return nil, newFault(protocol.ErrorCodeRevisionConflict, "local model assets changed")
	}
	if entry, present := engines[resource.ID]; present && !entry.resource.Stamp.Equal(resource.Stamp) {
		release(resource.ID)
	}
	if entry, present := engines[resource.ID]; present {
		return entry, nil
	}
	var bytes uint64
	for _, asset := range resource.Definition.Assets {
		bytes += asset.Bytes
	}
	// Make room by releasing the least recently used engines.
	for len(engines) > 0 {
		total, oldest := bytes, ""
		for id, entry := range engines {
			total += entry.bytes
			if oldest == "" || entry.used.Before(engines[oldest].used) || entry.used.Equal(engines[oldest].used) && id < oldest {
				oldest = id
			}
		}
		if total <= memoryBytes {
			break
		}
		release(oldest)
	}
	kind := Kind(resource.Definition.Format)
	if err := scheduler.capabilities.Ensure(kind); err != nil {
		return nil, err
	}
	engine, err := scheduler.capabilities[kind].Load(resource, item.validating)
	if err != nil {
		return nil, fault(err)
	}
	entry := &loaded{resource: resource, used: scheduler.clock.Now(), bytes: bytes, engine: engine}
	engines[resource.ID] = entry
	return entry, nil
}
