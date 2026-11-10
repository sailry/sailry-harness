package onnx

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

// openLibrary resolves dependencies only beside the verified file and in System32.
func openLibrary(path string) (uintptr, error) {
	handle, err := windows.LoadLibraryEx(path, 0, windows.LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|windows.LOAD_LIBRARY_SEARCH_SYSTEM32)
	return uintptr(handle), err
}

func librarySymbol(handle uintptr, name string) (uintptr, error) {
	return windows.GetProcAddress(windows.Handle(handle), name)
}

// sessionPath encodes ORTCHAR_T, which is wchar_t on Windows.
func sessionPath(path string) *byte {
	wide, err := windows.UTF16FromString(path)
	if err != nil {
		// A path with an embedded NUL names no file; the session reports it.
		wide = []uint16{0}
	}
	return (*byte)(unsafe.Pointer(&wide[0]))
}
