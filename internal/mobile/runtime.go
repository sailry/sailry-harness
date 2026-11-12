package mobile

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"strconv"
	"sync"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type Callback func(uint64, []byte)

type Handle struct {
	Handle string `json:"handle"`
	Kind   string `json:"kind"`
}

type envelope struct {
	OK     bool            `json:"ok"`
	Value  any             `json:"value"`
	Error  *protocol.Fault `json:"error,omitempty"`
	Closed bool            `json:"closed,omitempty"`
	Count  string          `json:"count,omitempty"`
}

type object interface {
	invoke(context.Context, *Runtime, uint64, string, json.RawMessage) (any, error)
	close() error
}

type owned struct {
	object object
	parent uint64
	ctx    context.Context
	cancel context.CancelFunc
}

type operation struct {
	owner  uint64
	cancel context.CancelFunc
	done   chan struct{}
}

type Runtime struct {
	ctx      context.Context
	cancel   context.CancelFunc
	callback Callback
	mu       sync.Mutex
	next     uint64
	handles  map[uint64]*owned
	pending  map[uint64]*operation
	closed   bool
	workers  sync.WaitGroup
	emission sync.Mutex
	emitted  uint64
}

func New(callback Callback) *Runtime {
	ctx, cancel := context.WithCancel(context.Background())
	return &Runtime{
		ctx: ctx, cancel: cancel, callback: callback,
		handles: map[uint64]*owned{}, pending: map[uint64]*operation{},
	}
}

func (runtime *Runtime) Invoke(handle uint64, method string, data []byte, request uint64) bool {
	input := append(json.RawMessage{}, data...)
	runtime.mu.Lock()
	if runtime.closed || request == 0 || runtime.pending[request] != nil {
		runtime.mu.Unlock()
		return false
	}
	parent := runtime.ctx
	entry := runtime.handles[handle]
	if entry != nil {
		parent = entry.ctx
	}
	ctx, cancel := context.WithCancel(parent)
	current := &operation{owner: handle, cancel: cancel, done: make(chan struct{})}
	runtime.pending[request] = current
	runtime.workers.Add(1)
	runtime.mu.Unlock()
	go func() {
		defer runtime.workers.Done()
		defer close(current.done)
		defer cancel()
		var value any
		var err error
		switch {
		case handle == 0 && (method == "close" || method == "dispose"):
			// Handle 0 names the root; releasing it would release every handle.
			err = invalid("the root is closed with the bridge")
		case method == "close" || method == "dispose":
			err = runtime.release(handle, request)
		case handle == 0:
			value, err = runtime.create(ctx, method, input)
		case entry == nil:
			err = unavailable("native handle is closed")
		default:
			value, err = entry.object.invoke(ctx, runtime, handle, method, input)
		}
		runtime.emit(request, result(value, err))
		runtime.mu.Lock()
		delete(runtime.pending, request)
		runtime.mu.Unlock()
	}()
	return true
}

func (runtime *Runtime) Cancel(request uint64) {
	runtime.mu.Lock()
	pending := runtime.pending[request]
	runtime.mu.Unlock()
	if pending != nil {
		pending.cancel()
	}
}

func (runtime *Runtime) Close(request uint64) {
	runtime.mu.Lock()
	if runtime.closed {
		runtime.mu.Unlock()
		return
	}
	runtime.closed = true
	runtime.cancel()
	runtime.mu.Unlock()
	runtime.workers.Wait()
	_ = runtime.release(0, 0)
	runtime.emission.Lock()
	runtime.emitted++
	encoded, _ := json.Marshal(envelope{OK: true, Closed: true, Count: strconv.FormatUint(runtime.emitted, 10)})
	if runtime.callback != nil {
		runtime.callback(request, encoded)
	}
	runtime.emission.Unlock()
}

func (runtime *Runtime) add(parent uint64, kind string, value object, ctx context.Context, cancel context.CancelFunc) (Handle, error) {
	runtime.mu.Lock()
	valid := !runtime.closed && ctx.Err() == nil && (parent == 0 || runtime.handles[parent] != nil)
	if !valid {
		runtime.mu.Unlock()
		cancel()
		_ = value.close()
		return Handle{}, unavailable("native owner is closed")
	}
	runtime.next++
	handle := runtime.next
	runtime.handles[handle] = &owned{object: value, parent: parent, ctx: ctx, cancel: cancel}
	runtime.mu.Unlock()
	return Handle{Handle: strconv.FormatUint(handle, 10), Kind: kind}, nil
}

func (runtime *Runtime) lifetime(ctx context.Context, parent uint64) (context.Context, context.CancelFunc, func()) {
	runtime.mu.Lock()
	owner := runtime.ctx
	if entry := runtime.handles[parent]; entry != nil {
		owner = entry.ctx
	}
	runtime.mu.Unlock()
	lifetime, cancel := context.WithCancel(owner)
	stop := context.AfterFunc(ctx, cancel)
	return lifetime, cancel, func() {
		if !stop() || ctx.Err() != nil {
			cancel()
		}
	}
}

func (runtime *Runtime) release(handle, current uint64) error {
	runtime.mu.Lock()
	selected := map[uint64]*owned{}
	for id, entry := range runtime.handles {
		ancestor := id
		for ancestor != 0 && ancestor != handle {
			owner := runtime.handles[ancestor]
			if owner == nil {
				break
			}
			ancestor = owner.parent
		}
		if handle == 0 || ancestor == handle {
			selected[id] = entry
		}
	}
	var pending []*operation
	for request, item := range runtime.pending {
		if request != current && selected[item.owner] != nil {
			item.cancel()
			pending = append(pending, item)
		}
	}
	var ids []uint64
	for id, entry := range selected {
		entry.cancel()
		delete(runtime.handles, id)
		ids = append(ids, id)
	}
	runtime.mu.Unlock()
	sort.Slice(ids, func(left, right int) bool { return ids[left] > ids[right] })
	var failures []error
	for _, id := range ids {
		failures = append(failures, selected[id].object.close())
	}
	for _, item := range pending {
		<-item.done
	}
	return errors.Join(failures...)
}

func (runtime *Runtime) emit(request uint64, value envelope) {
	encoded, err := json.Marshal(value)
	if err != nil {
		encoded, _ = json.Marshal(result(nil, invalid("native result could not be encoded")))
	}
	runtime.emission.Lock()
	runtime.emitted++
	if runtime.callback != nil {
		runtime.callback(request, encoded)
	}
	runtime.emission.Unlock()
}

func result(value any, err error) envelope {
	if err == nil {
		return envelope{OK: true, Value: value}
	}
	var direct *protocol.Fault
	if errors.As(err, &direct) {
		return envelope{Error: direct}
	}
	var wrapped *link.FaultError
	if errors.As(err, &wrapped) {
		return envelope{Error: &wrapped.Fault}
	}
	var fault protocol.Fault
	if errors.As(err, &fault) {
		return envelope{Error: &fault}
	}
	code := protocol.ErrorCodeInternal
	if errors.Is(err, context.Canceled) {
		code = protocol.ErrorCodeCancelled
	}
	return envelope{Error: &protocol.Fault{Code: code, Message: err.Error()}}
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func arguments(data json.RawMessage, target any) error {
	if len(data) == 0 {
		data = json.RawMessage("{}")
	}
	if err := json.Unmarshal(data, target); err != nil {
		return invalid("invalid native operation arguments")
	}
	return nil
}

func encode(value any) (string, error) {
	data, err := json.Marshal(value)
	return string(data), err
}
