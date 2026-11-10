//go:build !windows

package decision

import "github.com/ebitengine/purego"

func openLibrary(path string) (uintptr, error) {
	return purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_LOCAL)
}

func librarySymbol(handle uintptr, name string) (uintptr, error) {
	return purego.Dlsym(handle, name)
}

func closeLibrary(handle uintptr) {
	_ = purego.Dlclose(handle)
}
