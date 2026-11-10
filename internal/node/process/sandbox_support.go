package process

import "runtime"

// SandboxSupported reports whether the compiled platform has a workspace sandbox.
// A missing sandbox launcher still fails execution without broadening access.
func SandboxSupported() bool {
	return runtime.GOOS == "darwin" || runtime.GOOS == "linux"
}
