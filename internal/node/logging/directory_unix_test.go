//go:build unix

package logging

import (
	"os"
	"path/filepath"
	"testing"
)

// Log directories an earlier release left at 0755 are tightened instead of
// silently disabling the log; a permissive profile root still refuses it.
func TestInstallTightensLogDirectories(t *testing.T) {
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(profile, 0o700); err != nil {
		t.Fatal(err)
	}
	logs := filepath.Join(profile, "logs")
	directory := filepath.Join(logs, "desktop")
	if err := os.MkdirAll(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{logs, directory} {
		if err := os.Chmod(path, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	closeLog, err := Install(profile, "desktop")
	if err != nil {
		t.Fatal(err)
	}
	closeLog()
	for _, path := range []string{logs, directory} {
		if info, err := os.Lstat(path); err != nil || info.Mode().Perm() != 0o700 {
			t.Fatalf("%s: %v, %v", path, info, err)
		}
	}

	if err := os.Chmod(profile, 0o755); err != nil {
		t.Fatal(err)
	}
	if closeLog, err := Install(profile, "desktop"); err == nil {
		closeLog()
		t.Fatal("a permissive profile was accepted")
	}
	if info, err := os.Lstat(profile); err != nil || info.Mode().Perm() != 0o755 {
		t.Fatalf("profile changed: %v, %v", info, err)
	}
}
