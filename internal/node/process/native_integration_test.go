//go:build integration && (darwin || linux)

package process

import (
	"context"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestNativeCommandLifecycle(t *testing.T) {
	ctx := context.Background()
	service, err := New(Options{Profile: t.TempDir(), Environment: NewEnvironment([]string{"PATH=/usr/bin:/bin", "HOME=" + t.TempDir()}), CaptureManaged: git.New(git.Options{}).CaptureManaged})
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close()
	root := t.TempDir()
	launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: root, Command: "printf stdout; printf stderr >&2; exit 7", Access: protocol.ProcessAccessUnrestricted, TimeoutMs: 10000}
	result, err := service.Execute(ctx, launch)
	if err != nil || result.Outcome.Kind != "exited" || result.Stdout.Text != "stdout" || result.Stderr.Text != "stderr" {
		t.Fatal(result, err)
	}
	code, ok := result.Outcome.Data.(int32)
	if !ok || code != 7 {
		t.Fatal(result.Outcome)
	}
	launch.ID, launch.Command, launch.Pty = protocol.NewRequestID(), "read value; printf 'received:%s' \"$value\"", true
	info, err := service.Start(launch)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = service.Write(info.Session, info.ID, "fixture\n"); err != nil {
		t.Fatal(err)
	}
	if err = service.Wait(ctx, info.Session, info.ID, 30000, "received:fixture"); err != nil {
		t.Fatal(err)
	}
	snapshot, err := service.Read(info.Session, info.ID)
	if err != nil {
		t.Fatal(err)
	}
	if snapshot.Stdout.Text == "" {
		t.Fatal("native PTY returned no output")
	}
	launch.ID, launch.Pty, launch.Command = protocol.NewRequestID(), false, "printf inside > allowed; printf outside > "+shellQuote(filepath.Join(t.TempDir(), "denied"))+" 2>/dev/null; test $? -ne 0"
	launch.Access = protocol.ProcessAccessWorkspace
	result, err = service.Execute(ctx, launch)
	if err != nil || result.Outcome.Kind != "exited" {
		t.Fatal(result, err)
	}
	code, ok = result.Outcome.Data.(int32)
	if !ok || code != 0 {
		t.Fatal("sandbox allowed outside write", result)
	}
	if value, err := os.ReadFile(filepath.Join(root, "allowed")); err != nil || string(value) != "inside" {
		t.Fatal(string(value), err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	launch.ID, launch.Access = protocol.NewRequestID(), protocol.ProcessAccessUnrestricted
	launch.Command = fmt.Sprintf("nc -z -w 1 127.0.0.1 %d", listener.Addr().(*net.TCPAddr).Port)
	result, err = service.Execute(ctx, launch)
	if err != nil || result.Outcome.Kind != "exited" {
		t.Fatal("network fixture baseline failed", result, err)
	}
	if code, ok := result.Outcome.Data.(int32); !ok || code != 0 {
		t.Fatal("network fixture baseline did not connect", result)
	}
	launch.ID, launch.Access = protocol.NewRequestID(), protocol.ProcessAccessWorkspace
	result, err = service.Execute(ctx, launch)
	if err != nil || result.Outcome.Kind != "exited" {
		t.Fatal(result, err)
	}
	if code, ok := result.Outcome.Data.(int32); !ok || code == 0 {
		t.Fatal("sandbox allowed network access", result)
	}

}

func shellQuote(value string) string { return "'" + strings.ReplaceAll(value, "'", "'\\''") + "'" }
