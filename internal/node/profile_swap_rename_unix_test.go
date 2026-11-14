//go:build !windows

package node

import (
	"os"
	"path/filepath"
	"testing"
)

func preventProfileRename(t *testing.T, profile string) func() {
	t.Helper()
	if os.Geteuid() == 0 {
		t.Skip("the superuser renames inside a read-only directory")
	}
	parent := filepath.Dir(profile)
	info, err := os.Stat(parent)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(parent, info.Mode().Perm()&^0o222); err != nil {
		t.Fatal(err)
	}
	restored := false
	release := func() {
		if !restored {
			restored = true
			if err := os.Chmod(parent, info.Mode().Perm()); err != nil {
				t.Error(err)
			}
		}
	}
	t.Cleanup(release)
	return release
}
