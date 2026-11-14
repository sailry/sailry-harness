package iroh

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"runtime"
	"sync"
	"sync/atomic"
	"unsafe"

	"github.com/ebitengine/purego"
)

var ErrClosed = errors.New("iroh resource is closed")

type abi struct {
	init                    func(uintptr) int32
	defaultRelayURLs        func() uintptr
	endpointNew             func(*byte, uintptr) uintptr
	endpointAddress         func(uint64) uintptr
	endpointClose           func(uint64) uintptr
	endpointOnline          func(uint64) uintptr
	endpointNetworkChange   func(uint64) uintptr
	endpointInsertRelay     func(uint64, *byte, uintptr) uintptr
	endpointRemoveRelay     func(uint64, *byte, uintptr) uintptr
	endpointRelayStatus     func(uint64) uintptr
	endpointWaitRelayStatus func(uint64, *byte, uintptr) uintptr
	endpointWaitAddress     func(uint64, *byte, uintptr) uintptr
	connect                 func(uint64, *byte, uintptr, *byte, uintptr) uintptr
	accept                  func(uint64) uintptr
	openBi                  func(uint64) uintptr
	acceptBi                func(uint64) uintptr
	sendWrite               func(uint64, *byte, uintptr) uintptr
	sendFinish              func(uint64) uintptr
	sendStopped             func(uint64) uintptr
	sendReset               func(uint64, uint64) uintptr
	receiveRead             func(uint64, uintptr) uintptr
	receiveStop             func(uint64, uint64) uintptr
	connectionInfo          func(uint64) uintptr
	connectionWaitPath      func(uint64) uintptr
	connectionClose         func(uint64, uint64, *byte, uintptr) uintptr
	streamDrop              func(uint64, uint64) uintptr
	cancel                  func(uintptr)
	free                    func(unsafe.Pointer, uintptr)
	debugCount              func(uintptr) uintptr
}

type completion struct {
	status uintptr
	data   []byte
}

var loaded atomic.Pointer[abi]
var loadMutex sync.Mutex
var loadedPath string
var callbackOnce sync.Once
var callbackAddress uintptr
var pendingMutex sync.Mutex
var pending = make(map[uintptr]chan completion)

func Load(path string) error {
	loadMutex.Lock()
	defer loadMutex.Unlock()
	absolute, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	if runtime.GOOS == "android" && filepath.Base(path) == path {
		absolute = path
	}
	if loaded.Load() != nil {
		if loadedPath == absolute {
			return nil
		}
		return errors.New("iroh is already loaded from another library")
	}
	handle, err := openLibrary(absolute)
	if err != nil {
		return fmt.Errorf("load iroh library: %w", err)
	}
	functions := &abi{}
	for _, entry := range []struct {
		name   string
		target any
	}{
		{"init", &functions.init},
		{"default_relay_urls", &functions.defaultRelayURLs},
		{"endpoint_new", &functions.endpointNew},
		{"endpoint_addr", &functions.endpointAddress},
		{"endpoint_close", &functions.endpointClose},
		{"endpoint_online", &functions.endpointOnline},
		{"endpoint_network_change", &functions.endpointNetworkChange},
		{"endpoint_insert_relay", &functions.endpointInsertRelay},
		{"endpoint_remove_relay", &functions.endpointRemoveRelay},
		{"endpoint_relay_status", &functions.endpointRelayStatus},
		{"endpoint_wait_relay_status", &functions.endpointWaitRelayStatus},
		{"endpoint_wait_address", &functions.endpointWaitAddress},
		{"connect", &functions.connect},
		{"accept", &functions.accept},
		{"open_bi", &functions.openBi},
		{"accept_bi", &functions.acceptBi},
		{"send_write", &functions.sendWrite},
		{"send_finish", &functions.sendFinish},
		{"send_stopped", &functions.sendStopped},
		{"send_reset", &functions.sendReset},
		{"recv_read", &functions.receiveRead},
		{"recv_stop", &functions.receiveStop},
		{"conn_info", &functions.connectionInfo},
		{"conn_wait_path", &functions.connectionWaitPath},
		{"conn_close", &functions.connectionClose},
		{"stream_drop", &functions.streamDrop},
		{"cancel", &functions.cancel},
		{"free", &functions.free},
		{"debug_count", &functions.debugCount},
	} {
		address, err := librarySymbol(handle, "sailry_iroh_"+entry.name)
		if err != nil {
			closeLibrary(handle)
			return fmt.Errorf("resolve iroh %s: %w", entry.name, err)
		}
		purego.RegisterFunc(entry.target, address)
	}
	callbackOnce.Do(func() {
		callbackAddress = purego.NewCallback(complete)
	})
	loaded.Store(functions)
	if functions.init(callbackAddress) != 0 {
		loaded.Store(nil)
		closeLibrary(handle)
		return errors.New("initialize iroh runtime")
	}
	loadedPath = absolute
	return nil
}

// Windows callback thunks require a uintptr result. The native completion ABI
// returns void, so its caller ignores this adapter's zero result.
func complete(operation, status uintptr, pointer unsafe.Pointer, length uintptr) uintptr {
	var data []byte
	if pointer != nil {
		data = append([]byte(nil), unsafe.Slice((*byte)(pointer), int(length))...)
		loaded.Load().free(pointer, length)
	}
	pendingMutex.Lock()
	waiter := pending[operation]
	delete(pending, operation)
	pendingMutex.Unlock()
	if waiter != nil {
		waiter <- completion{status: status, data: data}
	}
	return 0
}

func invoke(ctx context.Context, schedule func(*abi) uintptr) (completion, error) {
	if err := ctx.Err(); err != nil {
		return completion{}, err
	}
	functions := loaded.Load()
	if functions == nil {
		return completion{}, errors.New("iroh library is not loaded")
	}
	waiter := make(chan completion, 1)
	pendingMutex.Lock()
	operation := schedule(functions)
	if operation == 0 {
		pendingMutex.Unlock()
		return completion{}, errors.New("iroh operation was not scheduled")
	}
	pending[operation] = waiter
	pendingMutex.Unlock()
	var result completion
	select {
	case result = <-waiter:
	case <-ctx.Done():
		functions.cancel(operation)
		result = <-waiter
	}
	switch result.status {
	case 0, 3:
		return result, nil
	case 2:
		if err := ctx.Err(); err != nil {
			return completion{}, err
		}
		return completion{}, ErrClosed
	default:
		var failure struct {
			Message string `json:"message"`
		}
		if err := json.Unmarshal(result.data, &failure); err != nil {
			return completion{}, fmt.Errorf("decode iroh error: %w", err)
		}
		return completion{}, errors.New(failure.Message)
	}
}

func invokeJSON(ctx context.Context, schedule func(*abi) uintptr, destination any) error {
	result, err := invoke(ctx, schedule)
	if err != nil {
		return err
	}
	if destination == nil {
		return nil
	}
	if err := json.Unmarshal(result.data, destination); err != nil {
		return fmt.Errorf("decode iroh result: %w", err)
	}
	return nil
}

func bytesPointer(data []byte) *byte {
	return unsafe.SliceData(data)
}
