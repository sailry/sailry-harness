package power

import (
	"sync"

	"github.com/ebitengine/purego"
)

// IOKit's idle-sleep assertion, through purego.
type assertion struct {
	id   uint32
	held bool
}

var iokit struct {
	once    sync.Once
	err     error
	create  func(kind uintptr, level uint32, name uintptr, id *uint32) int32
	release func(id uint32) int32
	text    func(allocator uintptr, text string, encoding uint32) uintptr
	free    func(value uintptr)
}

const (
	assertionLevelOn = 255
	encodingUTF8     = 0x08000100
	loadFailed       = -1
)

func load() error {
	iokit.once.Do(func() {
		foundation, err := purego.Dlopen("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			iokit.err = err
			return
		}
		library, err := purego.Dlopen("/System/Library/Frameworks/IOKit.framework/IOKit", purego.RTLD_NOW|purego.RTLD_LOCAL)
		if err != nil {
			iokit.err = err
			return
		}
		purego.RegisterLibFunc(&iokit.text, foundation, "CFStringCreateWithCString")
		purego.RegisterLibFunc(&iokit.free, foundation, "CFRelease")
		purego.RegisterLibFunc(&iokit.create, library, "IOPMAssertionCreateWithName")
		purego.RegisterLibFunc(&iokit.release, library, "IOPMAssertionRelease")
	})
	return iokit.err
}

func native() Backend { return &assertion{} }

func (a *assertion) Supported() bool { return true }
func (a *assertion) Held() bool      { return a.held }

func (a *assertion) Acquire() int64 {
	if load() != nil {
		return loadFailed
	}
	kind := iokit.text(0, "PreventUserIdleSystemSleep", encodingUTF8)
	reason := iokit.text(0, "Sailry", encodingUTF8)
	defer iokit.free(kind)
	defer iokit.free(reason)
	var id uint32
	if result := iokit.create(kind, assertionLevelOn, reason, &id); result != 0 {
		return int64(result)
	}
	a.id, a.held = id, true
	return 0
}

func (a *assertion) Release() int64 {
	if !a.held {
		return 0
	}
	if result := iokit.release(a.id); result != 0 {
		return int64(result)
	}
	a.held = false
	return 0
}
