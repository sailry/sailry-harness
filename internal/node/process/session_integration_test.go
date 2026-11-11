//go:build integration && (darwin || linux)

package process

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
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

func TestNativeOutcomes(t *testing.T) {
	t.Run("a stream held open outside the group keeps the exit status", func(t *testing.T) {
		service, launch := nativeFixture(t, Options{})
		// The detached child leaves the command's session, survives the group
		// kill and holds stdout past the drain bound, then exits on its own.
		launch.Command = `perl -MPOSIX -e 'fork and exit; POSIX::setsid(); sleep 4'; echo done; exit 3`
		result, err := service.Execute(context.Background(), launch)
		if err != nil || result.Outcome.Kind != "exited" {
			t.Fatal("held output replaced the exit status", result, err)
		}
		if code, ok := result.Outcome.Data.(int32); !ok || code != 3 {
			t.Fatal(result.Outcome)
		}
		// The child inherited both streams.
		if result.Stdout.Text != "done\n" || !result.Stdout.Truncated || !result.Stderr.Truncated {
			t.Fatal("held output was not marked as cut short", result)
		}
	})
	t.Run("a promoted command outlives its foreground timeout", func(t *testing.T) {
		service, launch := nativeFixture(t, Options{})
		launch.Command, launch.TimeoutMs = "sleep 1; echo finished", 200
		promotion := make(chan struct{})
		close(promotion)
		output, err := service.ExecutePromotable(context.Background(), launch, promotion)
		if err != nil || output.Kind != "command_started" {
			t.Fatal(output, err)
		}
		if err := service.Wait(context.Background(), launch.Session, launch.ID, 30000, "finished"); err != nil {
			t.Fatal(err)
		}
		if err := service.Wait(context.Background(), launch.Session, launch.ID, 30000, "missing"); err != nil {
			t.Fatal(err)
		}
		snapshot, err := service.Read(launch.Session, launch.ID)
		if err != nil || snapshot.Info.Status.Kind != "finished" {
			t.Fatal(snapshot, err)
		}
		if outcome, ok := snapshot.Info.Status.Data.(protocol.ProcessOutcome); !ok || outcome.Kind != "exited" {
			t.Fatal("promoted command kept its foreground timeout", snapshot.Info.Status)
		}
	})
	t.Run("a foreground command still times out", func(t *testing.T) {
		service, launch := nativeFixture(t, Options{})
		launch.Command, launch.TimeoutMs = "sleep 30", 200
		result, err := service.Execute(context.Background(), launch)
		if err != nil || result.Outcome.Kind != "timed_out" {
			t.Fatal(result, err)
		}
	})
	t.Run("an interactive command has a terminal type and 120 columns", func(t *testing.T) {
		service, launch := nativeFixture(t, Options{})
		launch.Command, launch.Pty = "echo term:$TERM; stty size; echo end", true
		info, err := service.Start(launch)
		if err != nil {
			t.Fatal(err)
		}
		if err := service.Wait(context.Background(), info.Session, info.ID, 30000, "end"); err != nil {
			t.Fatal(err)
		}
		snapshot, err := service.Read(info.Session, info.ID)
		if err != nil || !strings.Contains(snapshot.Stdout.Text, "term:xterm-256color") || !strings.Contains(snapshot.Stdout.Text, "24 120") {
			t.Fatal("interactive command terminal is not xterm-256color at 24x120", snapshot.Stdout.Text, err)
		}
	})
}

func TestCrashedNodeGroups(t *testing.T) {
	// orphan is a command group as a crashed Node leaves it: running, with
	// nothing left to stop it.
	orphan := func(t *testing.T) int {
		t.Helper()
		command := exec.Command("/bin/sh", "-c", "sleep 300 & wait")
		command.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
		if err := command.Start(); err != nil {
			t.Fatal(err)
		}
		group := command.Process.Pid
		t.Cleanup(func() { _ = syscall.Kill(-group, syscall.SIGKILL) })
		go func() { _ = command.Wait() }()
		return group
	}
	records := func(t *testing.T, profile string) []string {
		t.Helper()
		found, err := filepath.Glob(filepath.Join(profile, "tmp", groupPrefix+"*"))
		if err != nil {
			t.Fatal(err)
		}
		return found
	}
	t.Run("the next Node kills a recorded group", func(t *testing.T) {
		profile, group := t.TempDir(), orphan(t)
		recordGroup(profile, protocol.NewRequestID(), group)
		if len(records(t, profile)) != 1 {
			t.Fatal("running group was not recorded")
		}
		service, err := New(Options{Profile: profile})
		if err != nil {
			t.Fatal(err)
		}
		defer service.Close()
		groupGone(t, group)
		if len(records(t, profile)) != 0 {
			t.Fatal("recovered group kept its record")
		}
	})
	t.Run("a reused process ID is left alone", func(t *testing.T) {
		profile, group := t.TempDir(), orphan(t)
		recordGroup(profile, protocol.NewRequestID(), group)
		path := records(t, profile)[0]
		// The record names this process ID with another start time.
		if err := os.WriteFile(path, []byte(`{"group":`+strconv.Itoa(group)+`,"started":"another"}`), 0600); err != nil {
			t.Fatal(err)
		}
		service, err := New(Options{Profile: profile})
		if err != nil {
			t.Fatal(err)
		}
		defer service.Close()
		if err := syscall.Kill(-group, 0); err != nil {
			t.Fatal("recovery killed a process that only shares the recorded ID", err)
		}
		if len(records(t, profile)) != 0 {
			t.Fatal("stale record was kept")
		}
	})
	t.Run("a command is recorded only while it runs", func(t *testing.T) {
		service, launch := nativeFixture(t, Options{})
		group := startedGroup(t, service, launch)
		found := records(t, service.options.Profile)
		if len(found) != 1 || filepath.Base(found[0]) != groupPrefix+launch.ID.String() {
			t.Fatal("running command has no group record", found)
		}
		if _, err := service.Stop(launch.Session, launch.ID); err != nil {
			t.Fatal(err)
		}
		if err := service.Wait(context.Background(), launch.Session, launch.ID, 30000, "missing"); err != nil {
			t.Fatal(err)
		}
		groupGone(t, group)
		if len(records(t, service.options.Profile)) != 0 {
			t.Fatal("finished command kept its group record")
		}
	})
}
