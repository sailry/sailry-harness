package live

import (
	"runtime"
	"testing"
	"testing/synctest"
)

// MyGo's raster workers outlive the test and cannot share a WaitGroup with a
// synctest bubble. Paint on the caller until all bubble goroutines have exited.
func synctestWithRaster(t *testing.T, run func(*testing.T)) {
	t.Helper()
	// The processor count is process-wide, so disallow parallel tests and ancestors.
	t.Setenv("GOMAXPROCS", "1")
	previous := runtime.GOMAXPROCS(1)
	defer runtime.GOMAXPROCS(previous)
	synctest.Test(t, run)
}
