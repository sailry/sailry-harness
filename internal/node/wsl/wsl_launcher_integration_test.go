//go:build integration && !windows

package wsl

import (
	"context"
	"errors"
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
