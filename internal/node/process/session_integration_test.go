//go:build integration && (darwin || linux)

package process

import (
	"context"
	"errors"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func nativeFixture(t *testing.T, options Options) (*Service, Launch) {
	t.Helper()
	options.Profile, options.Environment = t.TempDir(), NewEnvironment([]string{"PATH=/usr/bin:/bin", "HOME=" + t.TempDir()})
	service, err := New(options)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	return service, Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: t.TempDir(), Access: protocol.ProcessAccessUnrestricted, TimeoutMs: 60000}
}

// startedGroup starts a background command that prints its process group and
// then keeps a child running.
func startedGroup(t *testing.T, service *Service, launch Launch) int {
	t.Helper()
	launch.Command = "echo group:$$; sleep 300 & wait"
	info, err := service.Start(launch)
	if err != nil {
		t.Fatal(err)
	}
	if err := service.Wait(context.Background(), info.Session, info.ID, 30000, "\n"); err != nil {
		t.Fatal(err)
	}
	snapshot, err := service.Read(info.Session, info.ID)
	if err != nil {
		t.Fatal(err)
	}
	text, found := strings.CutPrefix(strings.TrimSpace(snapshot.Stdout.Text), "group:")
	group, err := strconv.Atoi(text)
	if !found || err != nil || group <= 1 {
		t.Fatal("command did not report its process group", snapshot.Stdout.Text)
	}
	if err := syscall.Kill(-group, 0); err != nil {
		t.Fatal("command group is not running", err)
	}
	t.Cleanup(func() { _ = syscall.Kill(-group, syscall.SIGKILL) })
	return group
}

func groupGone(t *testing.T, group int) {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for !errors.Is(syscall.Kill(-group, 0), syscall.ESRCH) && !nativeGroupSettled(group) {
		if time.Now().After(deadline) {
			t.Fatal("command process group is still running")
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func TestSessionCommandsStop(t *testing.T) {
	t.Run("closing a session ends its process groups and entries", func(t *testing.T) {
		service, launch := nativeFixture(t, Options{})
		other := launch
		other.ID, other.Session = protocol.NewRequestID(), protocol.NewSessionID()
		group, kept := startedGroup(t, service, launch), startedGroup(t, service, other)
		if _, err := service.CloseSession(context.Background(), launch.Session); err != nil {
			t.Fatal(err)
		}
		groupGone(t, group)
		if len(service.List(launch.Session)) != 0 {
			t.Fatal("closed session kept command entries")
		}
		if err := syscall.Kill(-kept, 0); err != nil || len(service.List(other.Session)) != 1 {
			t.Fatal("closing a session touched another session's command", err)
		}
	})
}
