package node

import (
	"os"
	"testing"
)

func preventProfileRename(t *testing.T, profile string) func() {
	t.Helper()
	// Pin the current directory: staging succeeds, but Windows refuses to
	// rename the open root, exercising the swap's rollback after staging.
	root, err := os.OpenRoot(profile)
	if err != nil {
		t.Fatal(err)
	}
	closed := false
	release := func() {
		if !closed {
			if err := root.Close(); err != nil {
				t.Error(err)
				return
			}
			closed = true
		}
	}
	t.Cleanup(release)
	return release
}
