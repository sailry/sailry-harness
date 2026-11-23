package application

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
)

const startupRelay = "/ip4/127.0.0.1/tcp/30001/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"

func TestStartupScope(t *testing.T) {
	t.Parallel()
	const secondRelay = "/ip4/127.0.0.1/udp/30002/quic-v1/p2p/12D3KooWDwTirQce1RRKnasT5fPVFgzXCy6SiRgSwrwPGLC7zE91"
	options, err := parseOptions([]string{"--relay", startupRelay, "--data-dir", "/profile", "--relay", secondRelay})
	if err != nil || !reflect.DeepEqual(options.Relays, []string{startupRelay, secondRelay}) || options.DataDir != "/profile" {
		t.Fatalf("repeated --relay parsed as %+v, %v", options, err)
	}
	for _, args := range [][]string{
		{"--relay"},
		{"--relay", "https://old-relay.example.test"},
		{"--relay", "/ip4/127.0.0.1/tcp/30001"},
	} {
		if _, err := parseOptions(args); err == nil {
			t.Errorf("accepted invalid relay arguments %v", args)
		}
	}
	for _, current := range []struct {
		name   string
		relays []string
		want   link.NetworkScope
	}{
		{"direct by default", nil, link.NetworkScope{Internet: true}},
		{"explicit relays", options.Relays, link.NetworkScope{Relays: options.Relays}},
	} {
		if got := startupScope(current.relays); !reflect.DeepEqual(*got, current.want) {
			t.Errorf("%s: %+v, want %+v", current.name, *got, current.want)
		}
	}
}

func TestDesktopStartupLogUnavailable(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := core.AcquireProfile(profile)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = owner.Close() })
	if err := os.WriteFile(filepath.Join(profile, "logs"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	stderr, err := os.CreateTemp(t.TempDir(), "stderr")
	if err != nil {
		t.Fatal(err)
	}
	previous := os.Stderr
	os.Stderr = stderr
	t.Cleanup(func() { os.Stderr = previous; _ = stderr.Close() })
	backend, err := openBackend(t.Context(), startupOptions{}, identity.Desktop{DefaultProfile: profile}, moduleapi.Assembly{})
	if backend != nil {
		_ = backend.Close()
		t.Fatal("started a second Node in the profile")
	}
	var inUse *core.ProfileInUseError
	if !errors.As(err, &inUse) {
		t.Fatalf("Node startup error was masked: %v", err)
	}
	written, err := os.ReadFile(stderr.Name())
	if err != nil {
		t.Fatal(err)
	}
	if text := string(written); !strings.Contains(text, "sailry-desktop: continuing without a diagnostic log: logging is unavailable") || strings.Contains(text, profile) {
		t.Fatalf("startup diagnostic = %q", text)
	}
}

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
		backend, err := openBackend(context.Background(), options, identity.Desktop{DefaultProfile: path}, moduleapi.Assembly{})
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

func TestAcceptanceStartupIsolation(t *testing.T) {
	t.Parallel()
	for _, args := range [][]string{
		{"--acceptance-ui"},
		{"--acceptance-dir", "/test"},
		{"--acceptance-socket", "/control"},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--data-dir", "/profile"},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--preview"},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--relay", startupRelay},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--acceptance-mode", "ui", "--acceptance-socket", "/control"},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--acceptance-mode", "replay"},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--acceptance-mode", "replay", "--acceptance-replay", "/recordings", "--acceptance-socket", "/control"},
		{"--acceptance-ui", "--acceptance-dir", "/test", "--acceptance-mode", "e2e"},
	} {
		if _, err := parseOptions(args); err == nil {
			t.Fatalf("accepted ambiguous test startup: %v", args)
		}
	}
	options, err := parseOptions([]string{"--acceptance-ui", "--acceptance-dir", "/test", "--acceptance-mode", "full", "--acceptance-socket", "/control"})
	if err != nil || !options.AcceptanceUI || options.AcceptanceDirectory != "/test" || options.AcceptanceSocket != "/control" || options.DataDir != "" {
		t.Fatalf("isolated test startup: %+v, %v", options, err)
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
			message, detail := startFailureCopy(item.lang, "Sailry", item.failure)
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

func TestProfileRestart(t *testing.T) {
	t.Parallel()
	for _, item := range []struct {
		name              string
		check, stop, swap error
		confirmed         bool
		refused, relaunch bool
		failure           error
		steps             string
	}{
		{name: "a restored profile that cannot be used changes nothing", check: errors.New("no database"), confirmed: true, refused: true, steps: "check"},
		{name: "a restart that is not confirmed changes nothing", steps: "check confirm"},
		{name: "the profiles are swapped once the Node has stopped", confirmed: true, relaunch: true, steps: "check confirm stop swap relaunch"},
		{name: "a failed swap is shown before the restart", swap: errors.New("rename failed"), confirmed: true, relaunch: true, failure: errors.New("rename failed"), steps: "check confirm stop swap failed relaunch"},
		{name: "a Node that did not stop keeps its profile", stop: errors.New("stop failed"), confirmed: true, relaunch: true, failure: errors.New("stop failed"), steps: "check confirm stop failed relaunch"},
	} {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			var steps []string
			done := make(chan struct{})
			var failure error
			restart := &profileRestart{
				profile: "/profiles/main",
				check: func(_ context.Context, current, restored string) error {
					steps = append(steps, "check")
					if current != "/profiles/main" || restored != "/profiles/restored" {
						t.Errorf("checked %q against %q", restored, current)
					}
					return item.check
				},
				stop: func() error { steps = append(steps, "stop"); return item.stop },
				swap: func(_ context.Context, current, restored string, _ time.Time) (string, error) {
					steps = append(steps, "swap")
					return current + ".before-restore", item.swap
				},
				main:     func(run func()) { run() },
				failed:   func(err error) { steps = append(steps, "failed"); failure = err },
				relaunch: func() { steps = append(steps, "relaunch"); close(done) },
			}
			refused := false
			restart.run("/profiles/restored", func(error) { refused = true; close(done) }, func(proceed func()) {
				steps = append(steps, "confirm")
				if !item.confirmed {
					close(done)
					return
				}
				proceed()
			})
			<-done
			if strings.Join(steps, " ") != item.steps || refused != item.refused {
				t.Fatalf("steps %q, refused %v", steps, refused)
			}
			if (failure == nil) != (item.failure == nil) || failure != nil && failure.Error() != item.failure.Error() {
				t.Fatalf("shown failure = %v", failure)
			}
			// The Node is stopped once: a second request does nothing.
			if item.relaunch {
				restart.replace("/profiles/restored")
				if strings.Join(steps, " ") != item.steps {
					t.Fatalf("a second replacement ran: %q", steps)
				}
			}
		})
	}
}
