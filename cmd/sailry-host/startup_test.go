package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
)

func TestHostStartupLogUnavailable(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := core.AcquireProfile(profile)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Close() })
	if err := os.WriteFile(filepath.Join(profile, "logs"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	for _, unavailable := range []bool{false, true} {
		name := "stderr available"
		if unavailable {
			name = "stderr closed"
		}
		t.Run(name, func(t *testing.T) {
			stderr, err := os.CreateTemp(t.TempDir(), "stderr")
			if err != nil {
				t.Fatal(err)
			}
			previous := os.Stderr
			os.Stderr = stderr
			t.Cleanup(func() { os.Stderr = previous; _ = stderr.Close() })
			if unavailable {
				if err := stderr.Close(); err != nil {
					t.Fatal(err)
				}
			}
			code, err := run([]string{"serve", "--data-dir", profile})
			var inUse *core.ProfileInUseError
			if code != 1 || !errors.As(err, &inUse) {
				t.Fatalf("Node startup error was masked: exit %d: %v", code, err)
			}
			if !unavailable {
				written, err := os.ReadFile(stderr.Name())
				if err != nil {
					t.Fatal(err)
				}
				if text := string(written); !strings.Contains(text, "sailry-host: continuing without a diagnostic log: logging is unavailable") || strings.Contains(text, profile) {
					t.Fatalf("startup diagnostic = %q", text)
				}
			}
		})
	}
}
