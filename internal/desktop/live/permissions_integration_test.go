//go:build integration

package live

import (
	"errors"
	"runtime"
	"testing"

	"github.com/egoist/mygo/plugins/permissionflow"
)

// TestPermissionsHeadlessCheck reads this process's grants without an
// application loop and without asking. Full Disk Access is left out: its
// check opens a protected file, which can list the test binary in the
// system's settings.
func TestPermissionsHeadlessCheck(t *testing.T) {
	for kind, names := range permissionKinds {
		if names.flow == permissionflow.FullDiskAccess {
			continue
		}
		status, err := permissionflow.Check(names.flow)
		t.Logf("%s: %s, %v -> %s", names.flow, status, err, permissionStatus(runtime.GOOS, kind, status, err))
		if runtime.GOOS == "darwin" && err != nil {
			t.Errorf("%s: %v", names.flow, err)
		}
		if runtime.GOOS == "linux" && !errors.Is(err, errors.ErrUnsupported) {
			t.Errorf("%s: %v", names.flow, err)
		}
	}
}
