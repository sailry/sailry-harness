//go:build darwin

package uitest

import "testing"

// Parallel is a no-op on darwin because mygo@v0.10.0/internal/text keeps a
// package-global CoreText runDelegates pinner that is not safe for concurrent
// headless text shaping across parallel tests.
func Parallel(*testing.T) {}
