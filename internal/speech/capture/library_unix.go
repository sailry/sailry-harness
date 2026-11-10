//go:build !windows

package capture

import "github.com/ebitengine/purego"

func openLibrary(path string) (uintptr, error) {
	return purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_LOCAL)
}

func lookup(handle uintptr, name string) (uintptr, error) {
	return purego.Dlsym(handle, name)
}
