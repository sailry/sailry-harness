//go:build cgo

package main

/*
#cgo CFLAGS: -I${SRCDIR}/dart
#include <stdint.h>
#include <stdlib.h>
#include "dart_api_dl.h"

// Posts [request, bytes] to the bridge's port. Dart copies the bytes before
// returning, and a port whose isolate has exited reports false instead of
// running code that no longer exists.
static inline int sailry_post(int64_t port, uint64_t request, uint8_t *data, uint64_t length) {
	Dart_CObject id, bytes, message;
	Dart_CObject *items[2] = {&id, &bytes};
	id.type = Dart_CObject_kInt64;
	id.value.as_int64 = (int64_t)request;
	bytes.type = Dart_CObject_kTypedData;
	bytes.value.as_typed_data.type = Dart_TypedData_kUint8;
	bytes.value.as_typed_data.length = (intptr_t)length;
	bytes.value.as_typed_data.values = data;
	message.type = Dart_CObject_kArray;
	message.value.as_array.length = 2;
	message.value.as_array.values = items;
	return Dart_PostCObject_DL(port, &message);
}

// Reports whether the port's isolate still receives messages.
static inline int sailry_alive(int64_t port) {
	Dart_CObject message;
	message.type = Dart_CObject_kNull;
	return Dart_PostCObject_DL(port, &message);
}
*/
import "C"

import (
	"sync"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/mobile"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type bridge struct {
	runtime *mobile.Runtime
	port    C.int64_t
	closing chan struct{}
}

var bridges = struct {
	sync.Mutex
	apiInitialized bool
	next           uint64
	values         map[uint64]*bridge
}{values: map[uint64]*bridge{}}

// sailry_open starts a bridge whose results are posted to a Dart port.
//
// A Flutter engine can exit without closing its bridge, for example when
// Android finishes the activity, while the process and this runtime live on.
// Results for an exited isolate are dropped, and its runtime is closed when a
// post fails or the next bridge opens, so its controller and connections do
// not outlive their owner.
//
//export sailry_open
func sailry_open(api unsafe.Pointer, port C.int64_t) C.uint64_t {
	if api == nil {
		return 0
	}
	bridges.Lock()
	defer bridges.Unlock()
	if !bridges.apiInitialized {
		if C.Dart_InitializeApiDL(api) != 0 {
			return 0
		}
		bridges.apiInitialized = true
	}
	var predecessors []<-chan struct{}
	for id, existing := range bridges.values {
		if existing.closing == nil && C.sailry_alive(existing.port) == 0 {
			existing.closeLocked(id, 0)
		}
		if existing.closing != nil {
			predecessors = append(predecessors, existing.closing)
		}
	}
	bridges.next++
	handle := bridges.next
	runtime := mobile.New(func(request uint64, data []byte) {
		var bytes *C.uint8_t
		if len(data) > 0 {
			bytes = (*C.uint8_t)(unsafe.Pointer(&data[0]))
		}
		if C.sailry_post(port, C.uint64_t(request), bytes, C.uint64_t(len(data))) == 0 {
			closeBridge(handle, 0)
		}
	}, predecessors...)
	bridges.values[handle] = &bridge{runtime: runtime, port: port}
	return C.uint64_t(handle)
}

//export sailry_invoke
func sailry_invoke(bridge, handle C.uint64_t, method *C.char, input *C.uint8_t, length, request C.uint64_t, immediate **C.uint8_t, immediateLength *C.uint64_t) C.uint64_t {
	if immediate == nil || immediateLength == nil {
		return 0
	}
	*immediate, *immediateLength = nil, 0
	if method == nil {
		return 0
	}
	name := C.GoString(method)
	data, valid := copyInput(input, length, C.uint64_t(protocol.MAX_FRAME_BYTES))
	if !valid {
		return 0
	}
	if uint64(handle) == 0 {
		if encoded, synchronous := mobile.Sync(name, data); synchronous {
			*immediate = (*C.uint8_t)(C.CBytes(encoded))
			*immediateLength = C.uint64_t(len(encoded))
			return 0
		}
	}
	runtime := getBridge(bridge)
	if runtime == nil || !runtime.Invoke(uint64(handle), name, data, uint64(request)) {
		return 0
	}
	return request
}

//export sailry_subscribe
func sailry_subscribe(bridge, handle C.uint64_t, method *C.char, input *C.uint8_t, length, request C.uint64_t) C.uint64_t {
	data, valid := copyInput(input, length, protocol.MAX_FRAME_BYTES)
	runtime := getBridge(bridge)
	if !valid || runtime == nil || method == nil || !runtime.Invoke(uint64(handle), C.GoString(method), data, uint64(request)) {
		return 0
	}
	return request
}

//export sailry_cancel
func sailry_cancel(bridge, request C.uint64_t) {
	if runtime := getBridge(bridge); runtime != nil {
		runtime.Cancel(uint64(request))
	}
}

//export sailry_free
func sailry_free(data *C.uint8_t) {
	C.free(unsafe.Pointer(data))
}

//export sailry_close
func sailry_close(handle, request C.uint64_t) {
	closeBridge(uint64(handle), uint64(request))
}

func closeBridge(handle, request uint64) {
	bridges.Lock()
	if entry := bridges.values[handle]; entry != nil {
		entry.closeLocked(handle, request)
	}
	bridges.Unlock()
}

// Keep closing entries visible until their profile locks are released.
func (entry *bridge) closeLocked(handle, request uint64) {
	if entry.closing != nil {
		return
	}
	done := make(chan struct{})
	entry.closing = done
	go func() {
		entry.runtime.Close(request)
		bridges.Lock()
		delete(bridges.values, handle)
		close(done)
		bridges.Unlock()
	}()
}

func getBridge(handle C.uint64_t) *mobile.Runtime {
	bridges.Lock()
	defer bridges.Unlock()
	if entry := bridges.values[uint64(handle)]; entry != nil && entry.closing == nil {
		return entry.runtime
	}
	return nil
}

func copyInput(data *C.uint8_t, length, limit C.uint64_t) ([]byte, bool) {
	if length > limit || data == nil && length != 0 {
		return nil, false
	}
	return C.GoBytes(unsafe.Pointer(data), C.int(length)), true
}
