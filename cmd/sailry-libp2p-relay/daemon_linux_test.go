//go:build linux && !android

package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLinuxRelayManagementLock(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "lock")
	other := filepath.Join(root, "other")
	if err := os.Symlink(other, path); err != nil {
		t.Fatal(err)
	}
	if lock, err := lockRelayDaemon(path, os.Geteuid()); err == nil {
		lock.Close()
		t.Fatal("management lock followed a symlink")
	}
	assertDaemonTestMissing(t, other)
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	first, err := lockRelayDaemon(path, os.Geteuid())
	if err != nil {
		t.Fatal(err)
	}
	defer first.Close()
	if second, err := lockRelayDaemon(path, os.Geteuid()); err == nil {
		second.Close()
		t.Fatal("management commands acquired the same lock")
	} else if !strings.Contains(err.Error(), "in progress") {
		t.Fatal(err)
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	third, err := lockRelayDaemon(path, os.Geteuid())
	if err != nil {
		t.Fatal(err)
	}
	defer third.Close()
}
