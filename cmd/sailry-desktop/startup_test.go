package main

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestPreviewFlagNoNode(t *testing.T) {
	t.Parallel()
	for _, explicit := range []bool{false, true} {
		path := filepath.Join(t.TempDir(), "must-stay-absent")
		args := []string{"--preview"}
		if explicit {
			args = append(args, "--data-dir", path)
		}
		options, err := parseOptions(args)
		if err != nil {
			t.Fatal(err)
		}
		backend, err := openBackend(context.Background(), options)
		if err != nil {
			t.Fatal(err)
		}
		if backend.Node != nil || backend.Client != nil || backend.Preferences != nil || backend.Path != "" {
			t.Fatal("preview opened live state")
		}
		if err := backend.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(path); !os.IsNotExist(err) {
			t.Fatalf("preview changed profile path: %v", err)
		}
	}
}
