package main

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/identity"
)

func TestHostDefaultProfileIgnoresSharedBuildMarker(t *testing.T) {
	previous := core.Default
	t.Cleanup(func() { core.Default = previous })
	for _, shared := range []string{identity.SourceProfile, identity.InstalledProfile} {
		t.Run(shared, func(t *testing.T) {
			core.Default = shared
			output, err := os.CreateTemp(t.TempDir(), "stdout")
			if err != nil {
				t.Fatal(err)
			}
			previous := os.Stdout
			os.Stdout = output
			t.Cleanup(func() { os.Stdout = previous; _ = output.Close() })
			code, err := run([]string{"serve", "--help"})
			if code != 0 || err != nil {
				t.Fatalf("Host help = %d, %v", code, err)
			}
			written, err := os.ReadFile(output.Name())
			if err != nil || !strings.Contains(string(written), "Defaults to ~/.sailry-harness and loopback.") {
				t.Fatalf("Host help with shared %q = %q, %v", shared, written, err)
			}
		})
	}
}

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
