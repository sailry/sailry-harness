package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
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

func TestStartFailureCopy(t *testing.T) {
	t.Parallel()
	owned := fmt.Errorf("open profile: %w", &core.ProfileInUseError{Path: "/profiles/main"})
	for _, item := range []struct {
		name            string
		lang            string
		failure         error
		message, detail string
	}{
		{"a profile another process owns is named", "en", owned, "Sailry is already running with this data folder", "/profiles/main"},
		{"the message follows the system language", "zh-CN", owned, "Sailry 已在使用此数据目录运行", "/profiles/main"},
		{"any other failure shows its reason", "en", errors.New("Node database is not in WAL mode"), "Sailry could not start", "Node database is not in WAL mode"},
	} {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			message, detail := startFailureCopy(item.lang, item.failure)
			if message != item.message || detail != item.detail {
				t.Fatalf("start failure copy: %q, %q", message, detail)
			}
		})
	}
}

func TestInstanceLock(t *testing.T) {
	t.Parallel()
	for _, item := range []struct {
		name      string
		answers   []bool // of each request, the last one repeating
		successor bool
		locked    bool
		requests  int
	}{
		{"the first Desktop owns the lock", []bool{true}, false, true, 1},
		{"a second launch is handed over and exits", []bool{false}, false, false, 1},
		{"a successor waits for the Desktop it replaces to quit", []bool{false, false, false, true}, true, true, 4},
		{"a successor beside a Desktop that stays gives up", []bool{false}, true, false, successorAttempts + 1},
	} {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			requests, pauses := 0, 0
			locked := instanceLock(func() bool {
				answer := item.answers[min(requests, len(item.answers)-1)]
				requests++
				return answer
			}, item.successor, func() { pauses++ })
			if locked != item.locked || requests != item.requests || pauses != item.requests-1 {
				t.Fatalf("locked %v after %d requests and %d pauses", locked, requests, pauses)
			}
		})
	}
}
