package computer

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestComputerHelperLocator(t *testing.T) {
	t.Run("locate", func(t *testing.T) {
		bundled := filepath.Join("Sailry Computer Use.app", "Contents", "MacOS", "agent-desktop")
		for _, test := range []struct {
			goos, executable, helpers, expected string
		}{
			{"darwin", "/Applications/Sailry.app/Contents/MacOS/sailry-desktop", "", filepath.Join("/Applications/Sailry.app/Contents/Helpers", bundled)},
			{"darwin", "/opt/sailry/sailry-host", "", filepath.Join("/opt/sailry/helpers", bundled)},
			{"darwin", "/work/MacOS/sailry-desktop", "", filepath.Join("/work/MacOS/helpers", bundled)},
			{"darwin", "/Applications/Sailry.app/Contents/MacOS/sailry-desktop", "/staged", filepath.Join("/staged", bundled)},
			{"windows", "/opt/sailry/sailry-desktop.exe", "", "/opt/sailry/helpers/agent-desktop.exe"},
			{"linux", "/opt/sailry/sailry-host", "", "/opt/sailry/helpers/agent-desktop"},
			{"linux", "/Applications/Sailry.app/Contents/MacOS/sailry-host", "", "/Applications/Sailry.app/Contents/MacOS/helpers/agent-desktop"},
			{"linux", "/opt/sailry/sailry-host", "/staged", "/staged/agent-desktop"},
			{"freebsd", "/opt/sailry/sailry-host", "", ""},
		} {
			path, err := locate(test.goos, filepath.FromSlash(test.executable), filepath.FromSlash(test.helpers))
			if test.expected == "" {
				if !faulted(err, protocol.ErrorCodeUnavailable, "computer use is unavailable on this platform") {
					t.Errorf("%s: got %q, %v", test.goos, path, err)
				}
				continue
			}
			if err != nil || path != filepath.FromSlash(test.expected) {
				t.Errorf("%s %s %q: got %q, %v", test.goos, test.executable, test.helpers, path, err)
			}
		}
	})
	t.Run("validate", func(t *testing.T) {
		root := t.TempDir()
		helper := filepath.Join(root, "agent-desktop")
		if err := validate(helper); !faulted(err, protocol.ErrorCodeUnavailable, "computer helper is missing") {
			t.Fatal(err)
		}
		if err := validate(root); !faulted(err, protocol.ErrorCodeUnavailable, "computer helper is not a regular executable") {
			t.Fatal(err)
		}
		if err := os.WriteFile(helper, nil, 0o600); err != nil {
			t.Fatal(err)
		}
		if runtime.GOOS != "windows" {
			if err := validate(helper); !faulted(err, protocol.ErrorCodeUnavailable, "computer helper is not executable") {
				t.Fatal(err)
			}
			if err := os.Chmod(helper, 0o700); err != nil {
				t.Fatal(err)
			}
		}
		if err := validate(helper); err != nil {
			t.Fatal(err)
		}
		// The staged directory replaces the bundled location; PATH is never searched.
		t.Setenv("SAILRY_HELPERS_DIR", filepath.Join(root, "absent"))
		t.Setenv("PATH", root)
		if _, err := Executable(); !faulted(err, protocol.ErrorCodeUnavailable, "") {
			t.Fatal(err)
		}
	})
	t.Run("session", func(t *testing.T) {
		env := map[string]string{"AGENT_DESKTOP_HOME": "/data", "DISPLAY": ":7"}
		inherit(env, func(name string) (string, bool) {
			value, found := map[string]string{
				"DISPLAY": ":0", "WAYLAND_DISPLAY": "wayland-0", "XDG_RUNTIME_DIR": "/run/user/1000", "SAILRY_PRIVATE": "ignored",
			}[name]
			return value, found
		})
		expected := map[string]string{"AGENT_DESKTOP_HOME": "/data", "DISPLAY": ":7", "WAYLAND_DISPLAY": "wayland-0", "XDG_RUNTIME_DIR": "/run/user/1000"}
		if !reflect.DeepEqual(env, expected) {
			t.Fatal(env)
		}
	})
	t.Run("home", func(t *testing.T) {
		data := t.TempDir()
		home := filepath.Join(data, "agent-desktop")
		if err := PrepareHome(data, home); err != nil {
			t.Fatal(err)
		}
		if runtime.GOOS != "windows" {
			for _, directory := range []string{data, home} {
				if metadata, err := os.Stat(directory); err != nil || metadata.Mode().Perm() != 0o700 {
					t.Fatal(directory, metadata, err)
				}
			}
		}
		file := filepath.Join(data, "file")
		if err := os.WriteFile(file, nil, 0o600); err != nil {
			t.Fatal(err)
		}
		if PrepareHome(data, file) == nil {
			t.Fatal("a file was accepted as the helper home")
		}
	})
	t.Run("permissions", func(t *testing.T) {
		node, other := protocol.NodeID{1}, protocol.NodeID{2}
		report := func(accessibility, screen string) []byte {
			return fmt.Appendf(nil, `{"version":"2.6","ok":true,"command":"permissions","data":{"accessibility":{"state":%q},"screen_recording":{"state":%q}}}`, accessibility, screen)
		}
		for _, test := range []struct {
			body                  []byte
			screen, accessibility string
			malformed             bool
		}{
			{report("granted", "denied"), "false", "true", false},
			{report("not_required", "unknown"), "nil", "nil", false},
			{report("granted", "later"), "", "", true},
			{[]byte(`{"ok":false,"command":"permissions","data":null}`), "", "", true},
			{[]byte(`{"ok":true,"command":"version","data":{}}`), "", "", true},
			{[]byte(`{"ok":true,"command":"permissions","data":{"accessibility":{"state":"granted"}}}`), "", "", true},
			{[]byte(`private`), "", "", true},
		} {
			screen, accessibility, err := parse(test.body)
			if test.malformed {
				if !faulted(err, protocol.ErrorCodeUnavailable, "computer permission diagnostics are malformed") {
					t.Errorf("%s: %v", test.body, err)
				}
			} else if err != nil || grant(screen) != test.screen || grant(accessibility) != test.accessibility {
				t.Errorf("%s: %s %s %v", test.body, grant(screen), grant(accessibility), err)
			}
		}
		var calls []bool
		states := [][]byte{report("denied", "granted"), nil, report("granted", "granted")}
		probe := Probe{goos: "darwin", run: func(_ context.Context, _ Invocation, request bool) ([]byte, error) {
			calls = append(calls, request)
			return states[len(calls)-1], nil
		}}
		launch := &Invocation{Command: "agent-desktop"}
		if _, err := probe.Read(t.Context(), node, node, nil); !faulted(err, protocol.ErrorCodeNotConfigured, "") {
			t.Fatal(err)
		}
		if _, err := probe.Request(t.Context(), node, other, protocol.ComputerPermissionAccessibility, launch); !faulted(err, protocol.ErrorCodePermissionDenied, "") {
			t.Fatal(err)
		}
		// A granted permission is reported without prompting for the other one.
		value, err := probe.Request(t.Context(), node, node, protocol.ComputerPermissionScreenCapture, launch)
		if err != nil || !value.Local || value.Platform != "macos" || grant(value.Accessibility) != "false" || !reflect.DeepEqual(calls, []bool{false}) {
			t.Fatal(value, calls, err)
		}
		calls = nil
		value, err = probe.Request(t.Context(), node, node, protocol.ComputerPermissionAccessibility, launch)
		if err != nil || grant(value.Accessibility) != "true" || !reflect.DeepEqual(calls, []bool{false, true, false}) {
			t.Fatal(value, calls, err)
		}
		linux := Probe{goos: "linux", run: func(context.Context, Invocation, bool) ([]byte, error) {
			t.Fatal("the helper ran outside macOS")
			return nil, nil
		}}
		value, err = linux.Read(t.Context(), node, other, nil)
		if err != nil || value.Local || value.Platform != "linux" || value.ScreenCapture != nil || value.Accessibility != nil {
			t.Fatal(value, err)
		}
		if _, err := linux.Request(t.Context(), node, node, protocol.ComputerPermissionAccessibility, nil); !faulted(err, protocol.ErrorCodeUnavailable, "computer permission setup is unavailable on this platform") {
			t.Fatal(err)
		}
	})
	t.Run("overlay", func(t *testing.T) {
		home := filepath.Join(t.TempDir(), "agent-desktop")
		if err := os.MkdirAll(home, 0o700); err != nil {
			t.Fatal(err)
		}
		helper := &fakeHelper{home: home}
		overlay := &Overlay{run: helper.run}
		launch := Invocation{Command: "agent-desktop", Env: map[string]string{homeVariable: home}}
		attach := func(allowed bool) string {
			session, _ := overlay.Attach("computer", allowed, launch)
			return session
		}
		if attach(false) != "" || len(helper.calls) != 0 {
			t.Fatal(helper.calls)
		}
		declared := Invocation{Env: map[string]string{homeVariable: home, SessionVariable: "declared"}}
		if _, attached := overlay.Attach("computer", true, declared); attached || len(helper.calls) != 0 {
			t.Fatal(helper.calls)
		}
		if _, attached := overlay.Attach("computer", true, Invocation{}); attached || len(helper.calls) != 0 {
			t.Fatal(helper.calls)
		}
		if attach(true) != "run-1" || attach(true) != "run-1" {
			t.Fatal(helper.calls)
		}
		if !reflect.DeepEqual(helper.calls, []string{"session gc --ended", "session start --no-trace --name sailry"}) {
			t.Fatal(helper.calls)
		}
		var configured struct {
			Name    string `json:"name"`
			Overlay struct {
				Enabled bool `json:"enabled"`
				Style   struct {
					Size   float64 `json:"size"`
					Images struct {
						Arrow struct {
							Path    string         `json:"path"`
							Hotspot map[string]int `json:"hotspot"`
						} `json:"arrow"`
					} `json:"images"`
				} `json:"style"`
			} `json:"cursor_overlay"`
		}
		body, err := os.ReadFile(filepath.Join(home, "sessions", "run-1", "session.json"))
		if err != nil || json.Unmarshal(body, &configured) != nil || configured.Name != "sailry" || !configured.Overlay.Enabled {
			t.Fatal(string(body), err)
		}
		if image := configured.Overlay.Style.Images.Arrow; runtime.GOOS == "darwin" || runtime.GOOS == "windows" {
			written, err := os.ReadFile(image.Path)
			if err != nil || len(written) != len(cursor) || image.Hotspot["x"] != 16 || image.Path != filepath.Join(home, "sessions", "run-1", "cursor.png") {
				t.Fatal(image, err)
			}
		} else if configured.Overlay.Style.Size != 0.75 || image.Path != "" {
			t.Fatal(string(body))
		}
		// An ended session is replaced without collecting again.
		helper.manifest("run-1", true)
		if attach(true) != "run-2" || strings.Count(strings.Join(helper.calls, "\n"), "session gc") != 1 {
			t.Fatal(helper.calls)
		}
		// A failed end keeps the session for the next attempt.
		helper.endFails = true
		overlay.Retain("computer", nil)
		if attach(true) != "run-2" || helper.started != 2 {
			t.Fatal(helper.calls)
		}
		helper.endFails = false
		overlay.Retain("other", nil)
		if attach(true) != "run-2" {
			t.Fatal(helper.calls)
		}
		overlay.Retain("", []string{"other"})
		if helper.calls[len(helper.calls)-1] != "session end run-2" || active(home, "run-2") {
			t.Fatal(helper.calls)
		}
		// A failed or malformed start leaves the launch headless.
		for _, mode := range []string{"failed", "malformed"} {
			helper.mode = mode
			if attach(true) != "" {
				t.Fatal(mode)
			}
		}
		// An interrupted start ends only this owner's unrecorded sessions.
		helper.mode, helper.calls = "stalled", nil
		helper.list = `{"ok":true,"data":{"sessions":[{"session_id":"run-3","name":"sailry","ended_at":null},{"session_id":"manual","name":"operator","ended_at":null},{"session_id":"ended","name":"sailry","ended_at":1},{"session_id":"../other","name":"sailry","ended_at":null}]}}`
		if attach(true) != "" || !reflect.DeepEqual(helper.calls, []string{"session start --no-trace --name sailry", "session list", "session end run-3"}) {
			t.Fatal(helper.calls)
		}
		helper.mode = ""
		if attach(true) != "run-4" {
			t.Fatal(helper.calls)
		}
		overlay.Shutdown()
		if helper.calls[len(helper.calls)-1] != "session end run-4" || attach(true) != "" {
			t.Fatal(helper.calls)
		}
		if !OverlayEnabled(nil) || OverlayEnabled(map[string]json.RawMessage{"cursor_overlay": json.RawMessage("false")}) {
			t.Fatal("the overlay setting defaults on")
		}
		for _, data := range []string{`{}`, `{"session_id":""}`, `{"session_id":"../other"}`, `{"session_id":"` + strings.Repeat("run-", 20) + `"}`} {
			if _, code := sessionID([]byte(data)); code != "INVALID_SESSION" {
				t.Fatal(data)
			}
		}
	})
}

// fakeHelper answers the helper's session commands in process.
type fakeHelper struct {
	home     string
	mode     string
	list     string
	calls    []string
	started  int
	endFails bool
}

func (helper *fakeHelper) manifest(session string, ended bool) {
	directory := filepath.Join(helper.home, "sessions", session)
	_ = os.MkdirAll(directory, 0o700)
	body := fmt.Sprintf(`{"id":%q,"name":"sailry","created_at":1,"trace":"off","artifacts":"events"}`, session)
	if ended {
		body = fmt.Sprintf(`{"id":%q,"ended_at":1}`, session)
	}
	_ = os.WriteFile(filepath.Join(directory, "session.json"), []byte(body), 0o600)
}

func (helper *fakeHelper) run(_ context.Context, invocation Invocation, args []string) ([]byte, bool, error) {
	if _, inherited := invocation.Env[SessionVariable]; inherited {
		return nil, false, errors.New("the session variable reached a session command")
	}
	call := strings.Join(args, " ")
	helper.calls = append(helper.calls, call)
	switch {
	case call == "session gc --ended":
		return []byte(`{"ok":true,"command":"session","data":{"removed":[]}}` + "\n"), true, nil
	case call == "session start --no-trace --name sailry":
		switch helper.mode {
		case "failed":
			return []byte(`{"ok":false,"error":{"code":"INTERNAL","message":"private content"}}`), false, nil
		case "malformed":
			return []byte("private content"), true, nil
		}
		helper.started++
		session := fmt.Sprintf("run-%d", helper.started)
		helper.manifest(session, false)
		if helper.mode == "stalled" {
			return nil, false, context.DeadlineExceeded
		}
		return fmt.Appendf(nil, `{"ok":true,"command":"session","data":{"session_id":%q}}`+"\n", session), true, nil
	case call == "session list":
		return []byte(helper.list), true, nil
	case strings.HasPrefix(call, "session end "):
		if helper.endFails {
			return []byte(`{"ok":false,"error":{"code":"INTERNAL"}}`), false, nil
		}
		helper.manifest(args[2], true)
		return fmt.Appendf(nil, `{"ok":true,"command":"session","data":{"ended_at":1,"session_id":%q}}`+"\n", args[2]), true, nil
	}
	return nil, false, nil
}

func grant(value *bool) string {
	if value == nil {
		return "nil"
	}
	return fmt.Sprint(*value)
}

func faulted(err error, code protocol.ErrorCode, message string) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == code && (message == "" || fault.Message == message)
}
