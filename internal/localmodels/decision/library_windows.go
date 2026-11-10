package decision

import "golang.org/x/sys/windows"

// openLibrary resolves dependencies only beside the verified absolute file
// and in System32.
func openLibrary(path string) (uintptr, error) {
	handle, err := windows.LoadLibraryEx(path, 0, windows.LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|windows.LOAD_LIBRARY_SEARCH_SYSTEM32)
	return uintptr(handle), err
}

func librarySymbol(handle uintptr, name string) (uintptr, error) {
	return windows.GetProcAddress(windows.Handle(handle), name)
}

func closeLibrary(handle uintptr) {
	_ = windows.FreeLibrary(windows.Handle(handle))
}
