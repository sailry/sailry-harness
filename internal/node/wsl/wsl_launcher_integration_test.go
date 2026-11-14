//go:build integration && !windows

package wsl

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestWSLLauncher exercises the bounded launcher with a real POSIX shell.
func TestWSLLauncher(t *testing.T) {
	shell, arguments := "/bin/sh", []string{"-s"}
	environment := []string{"PATH=/usr/bin:/bin"}
	ctx := context.Background()
	if output, err := run(ctx, shell, arguments, environment, "printf ready", 5*time.Second); err != nil || output != "ready" {
		t.Fatalf("output %q, error %v", output, err)
	}
	var fault *protocol.Fault
	_, err := run(ctx, shell, arguments, environment, "echo 'systemd is unavailable' >&2; exit 1", 5*time.Second)
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.Message != "WSL command failed (1): systemd is unavailable" {
		t.Fatalf("failure: %v", err)
	}
	for _, script := range []string{"exit 73", "kill -KILL $$", "printf '\\377'", "head -c 131073 /dev/zero"} {
		if _, err := run(ctx, shell, arguments, environment, script, 5*time.Second); faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
			t.Errorf("%q: %v", script, err)
		}
	}
	if _, err := run(ctx, shell, arguments, environment, "exec sleep 30", 100*time.Millisecond); faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
		t.Errorf("timeout: %v", err)
	}
	_, err = query(ctx, shell, []string{"-c", "exec sleep 30"}, environment, 100*time.Millisecond)
	if !errors.As(err, &fault) || fault.Message != "WSL discovery timed out" {
		t.Errorf("discovery timeout: %v", err)
	}
	_, err = query(ctx, shell, []string{"-c", "exit 3"}, environment, 5*time.Second)
	if !errors.As(err, &fault) || fault.Message != "WSL discovery failed with exit code 3 (0x00000003)" {
		t.Errorf("discovery failure: %v", err)
	}
	_, err = query(ctx, shell, []string{"-c", "head -c 65537 /dev/zero"}, environment, 5*time.Second)
	if !errors.As(err, &fault) || fault.Message != "WSL discovery output exceeds the supported size" {
		t.Errorf("discovery overflow: %v", err)
	}
	if body, err := query(ctx, shell, []string{"-c", "if read -r line; then exit 77; fi; printf list"}, environment, 5*time.Second); err != nil || string(body) != "list" {
		t.Errorf("discovery output %q, error %v", body, err)
	}
	stopping, stop := context.WithCancel(ctx)
	timer := time.AfterFunc(100*time.Millisecond, stop)
	defer timer.Stop()
	if _, err := run(stopping, shell, arguments, environment, "exec sleep 30", 5*time.Second); faultCode(err) != protocol.ErrorCodeCancelled {
		t.Errorf("cancellation: %v", err)
	}
	if _, err := run(stopping, shell, arguments, environment, "exit 0", 5*time.Second); faultCode(err) != protocol.ErrorCodeCancelled {
		t.Errorf("cancelled before launch: %v", err)
	}
	if _, err := query(stopping, shell, []string{"-c", "exit 0"}, environment, 5*time.Second); faultCode(err) != protocol.ErrorCodeCancelled {
		t.Errorf("discovery cancelled before launch: %v", err)
	}
}

func TestWSLReleaseUpdateOutcome(t *testing.T) {
	for _, fixture := range []struct {
		name   string
		status int
		code   protocol.ErrorCode
	}{
		{"refused before changes", 1, protocol.ErrorCodeUnavailable},
		{"recovery unconfirmed", 73, protocol.ErrorCodeOutcomeUnknown},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			directory := t.TempDir()
			installer := []byte(fmt.Sprintf("#!/bin/sh\nexit %d\n", fixture.status))
			if err := os.WriteFile(filepath.Join(directory, "installer"), installer, 0o600); err != nil {
				t.Fatal(err)
			}
			checksum := fmt.Sprintf("%x  install-host.sh\n", sha256.Sum256(installer))
			if err := os.WriteFile(filepath.Join(directory, "checksums"), []byte(checksum), 0o600); err != nil {
				t.Fatal(err)
			}
			script, err := ReleaseScript("0.1.0", true, nil)
			if err != nil {
				t.Fatal(err)
			}
			// Substitute only the download with local files. The real
			// bootstrap still verifies the checksum and launches the update.
			const downloads = `curl() {
  for destination do :; done
  case "$destination" in
    */install-host.sh) cp "$SAILRY_FIXTURE/installer" "$destination" ;;
    */SHA256SUMS-installer) cp "$SAILRY_FIXTURE/checksums" "$destination" ;;
    *) return 1 ;;
  esac
}
`
			environment := []string{"PATH=/usr/bin:/bin", "TMPDIR=" + directory, "SAILRY_FIXTURE=" + directory}
			if _, err := run(t.Context(), "/bin/sh", []string{"-s"}, environment, downloads+script, 5*time.Second); faultCode(err) != fixture.code {
				t.Fatalf("release update outcome changed: %v", err)
			}
		})
	}
}
