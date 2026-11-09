//go:build windows

package reader

import (
	"fmt"
	"runtime"
	"syscall"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	speechOLE            = windows.NewLazySystemDLL("ole32.dll")
	speechCoInitialize   = speechOLE.NewProc("CoInitializeEx")
	speechCoCreate       = speechOLE.NewProc("CoCreateInstance")
	speechCoUninitialize = speechOLE.NewProc("CoUninitialize")
	speechClass          = windows.GUID{Data1: 0x96749377, Data2: 0x3391, Data3: 0x11d2, Data4: [8]byte{0x9e, 0xe3, 0x00, 0xc0, 0x4f, 0x79, 0x73, 0x96}}
	speechInterface      = windows.GUID{Data1: 0x6c44df74, Data2: 0x72b9, Data3: 0x4992, Data4: [8]byte{0xa1, 0xec, 0xef, 0x99, 0x6e, 0x04, 0x22, 0xd4}}
)

type nativeSpeech struct {
	voice       uintptr
	initialized bool
}

func newNativeSpeech() (*nativeSpeech, error) {
	result, _, _ := speechCoInitialize.Call(0, 2)
	initialized := int32(result) >= 0
	if !initialized && uint32(result) != 0x80010106 {
		return nil, speechHRESULT(result)
	}
	var voice uintptr
	result, _, _ = speechCoCreate.Call(uintptr(unsafe.Pointer(&speechClass)), 0, 1, uintptr(unsafe.Pointer(&speechInterface)), uintptr(unsafe.Pointer(&voice)))
	if int32(result) < 0 {
		if initialized {
			speechCoUninitialize.Call()
		}
		return nil, speechHRESULT(result)
	}
	return &nativeSpeech{voice: voice, initialized: initialized}, nil
}
func speechHRESULT(result uintptr) error {
	return fmt.Errorf("system speech operation failed (HRESULT 0x%08x)", uint32(result))
}
func (s *nativeSpeech) method(index int, args ...uintptr) uintptr {
	table := *(*uintptr)(unsafe.Pointer(s.voice))
	method := *(*uintptr)(unsafe.Add(unsafe.Pointer(table), index*int(unsafe.Sizeof(uintptr(0)))))
	result, _, _ := syscall.SyscallN(method, append([]uintptr{s.voice}, args...)...)
	return result
}
func (s *nativeSpeech) speak(text string) error {
	value, err := windows.UTF16PtrFromString(text)
	if err != nil {
		return err
	}
	result := s.method(20, uintptr(unsafe.Pointer(value)), 3, 0)
	runtime.KeepAlive(value)
	if int32(result) < 0 {
		return speechHRESULT(result)
	}
	return nil
}
func (s *nativeSpeech) stop() error {
	result := s.method(20, 0, 3, 0)
	if int32(result) < 0 {
		return speechHRESULT(result)
	}
	return nil
}
func (s *nativeSpeech) speaking() (bool, error) {
	result := s.method(32, 0)
	if int32(result) < 0 {
		return false, speechHRESULT(result)
	}
	return result == 1, nil
}
func (s *nativeSpeech) close() error {
	err := s.stop()
	s.method(2)
	s.voice = 0
	if s.initialized {
		speechCoUninitialize.Call()
	}
	return err
}
