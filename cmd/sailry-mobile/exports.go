//go:build cgo

package main

/*
#include <stdint.h>
#include <stdlib.h>
typedef void (*sailry_callback)(uint64_t request, uint8_t *data, uint64_t length);
static inline void sailry_complete(sailry_callback callback, uint64_t request, uint8_t *data, uint64_t length) {
	callback(request, data, length);
}
*/
import "C"

import (
	"sync"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/mobile"
)

var bridges = struct {
	sync.Mutex
	next   uint64
	values map[uint64]*mobile.Runtime
}{values: map[uint64]*mobile.Runtime{}}

//export sailry_open
func sailry_open(callback C.sailry_callback) C.uint64_t {
	if callback == nil {
		return 0
	}
	runtime := mobile.New(func(request uint64, data []byte) {
		buffer := C.CBytes(data)
		C.sailry_complete(callback, C.uint64_t(request), (*C.uint8_t)(buffer), C.uint64_t(len(data)))
	})
	bridges.Lock()
	bridges.next++
	handle := bridges.next
	bridges.values[handle] = runtime
	bridges.Unlock()
	return C.uint64_t(handle)
}

//export sailry_invoke
func sailry_invoke(bridge, handle C.uint64_t, method *C.char, input *C.uint8_t, length, request C.uint64_t, immediate **C.uint8_t, immediateLength *C.uint64_t) C.uint64_t {
	if immediate == nil || immediateLength == nil {
		return 0
	}
	*immediate, *immediateLength = nil, 0
	data, valid := copyInput(input, length)
	if !valid || method == nil {
		return 0
	}
	name := C.GoString(method)
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
	data, valid := copyInput(input, length)
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
func sailry_close(bridge, request C.uint64_t) {
	bridges.Lock()
	runtime := bridges.values[uint64(bridge)]
	delete(bridges.values, uint64(bridge))
	bridges.Unlock()
	if runtime != nil {
		go runtime.Close(uint64(request))
	}
}

func getBridge(handle C.uint64_t) *mobile.Runtime {
	bridges.Lock()
	defer bridges.Unlock()
	return bridges.values[uint64(handle)]
}

func copyInput(data *C.uint8_t, length C.uint64_t) ([]byte, bool) {
	if uint64(length) > 16*1024*1024 || data == nil && length != 0 {
		return nil, false
	}
	return C.GoBytes(unsafe.Pointer(data), C.int(length)), true
}
