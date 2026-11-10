package power

import (
	"syscall"
	"unsafe"
)

// A process-owned power request for PowerRequestSystemRequired.
type request struct {
	handle syscall.Handle
	open   bool
	active bool
}

var (
	kernel32           = syscall.NewLazyDLL("kernel32.dll")
	powerCreateRequest = kernel32.NewProc("PowerCreateRequest")
	powerSetRequest    = kernel32.NewProc("PowerSetRequest")
	powerClearRequest  = kernel32.NewProc("PowerClearRequest")
)

const (
	powerRequestContextVersion      = 0
	powerRequestContextSimpleString = 1
	powerRequestSystemRequired      = 1
)

// reasonContext is REASON_CONTEXT with its simple string.
type reasonContext struct {
	version uint32
	flags   uint32
	reason  *uint16
	_       [2]uintptr // the detailed reason's remaining fields
}

func native() Backend { return &request{} }

func (r *request) Supported() bool { return true }
func (r *request) Held() bool      { return r.active }

func code(err error) int64 {
	if errno, ok := err.(syscall.Errno); ok && errno != 0 {
		return int64(errno)
	}
	return -1
}

func (r *request) Acquire() int64 {
	reason, err := syscall.UTF16PtrFromString("Sailry")
	if err != nil {
		return -1
	}
	context := reasonContext{version: powerRequestContextVersion, flags: powerRequestContextSimpleString, reason: reason}
	handle, _, err := powerCreateRequest.Call(uintptr(unsafe.Pointer(&context)))
	if syscall.Handle(handle) == syscall.InvalidHandle || handle == 0 {
		return code(err)
	}
	r.handle, r.open = syscall.Handle(handle), true
	if ok, _, err := powerSetRequest.Call(handle, powerRequestSystemRequired); ok == 0 {
		failure := code(err)
		r.Release()
		return failure
	}
	r.active = true
	return 0
}

func (r *request) Release() int64 {
	if !r.open {
		return 0
	}
	var failure int64
	if r.active {
		if ok, _, err := powerClearRequest.Call(uintptr(r.handle), powerRequestSystemRequired); ok == 0 {
			failure = code(err)
		} else {
			r.active = false
		}
	}
	if err := syscall.CloseHandle(r.handle); err != nil {
		if failure == 0 {
			failure = code(err)
		}
	} else {
		r.open, r.active = false, false
	}
	return failure
}
