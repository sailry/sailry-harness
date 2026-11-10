//go:build !windows

package onnx

import "github.com/ebitengine/purego"

func openLibrary(path string) (uintptr, error) {
	return purego.Dlopen(path, purego.RTLD_NOW|purego.RTLD_LOCAL)
}

func librarySymbol(handle uintptr, name string) (uintptr, error) {
	return purego.Dlsym(handle, name)
}

// sessionPath encodes ORTCHAR_T, which is char outside Windows.
func sessionPath(path string) *byte { return cString(path) }
