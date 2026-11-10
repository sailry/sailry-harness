package speech

import "golang.org/x/sys/windows"

func openLibrary(path string) (uintptr, error) {
	// Resolve dependencies only beside the verified file and in System32.
	handle, err := windows.LoadLibraryEx(path, 0, windows.LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|windows.LOAD_LIBRARY_SEARCH_SYSTEM32)
	return uintptr(handle), err
}

func lookup(handle uintptr, name string) (uintptr, error) {
	return windows.GetProcAddress(windows.Handle(handle), name)
}
