//go:build !windows

package logging

import (
	"os"
	"testing"
)

func assertLogPrivacy(t *testing.T, path string) {
	t.Helper()
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0o600 {
		t.Fatalf("private log file: %v, %v", info, err)
	}
}
