//go:build !darwin

package uitest

import "testing"

// Parallel enables parallel test execution on platforms where mygo text
// shaping does not rely on CoreText's global run-delegate state.
func Parallel(t *testing.T) {
	t.Parallel()
}
