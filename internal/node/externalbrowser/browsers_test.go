package externalbrowser

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestCallsWithoutBrowser covers everything that is decided before a browser
// starts: argument and result bounds, the calls that never need a browser,
// leftovers of an unclean stop, and the single fault of a host without an
// installed browser. No browser is launched and nothing is downloaded.
func TestCallsWithoutBrowser(t *testing.T) {
	refused := func(t *testing.T, err error, code protocol.ErrorCode, message string) {
		t.Helper()
		var got *protocol.Fault
		if !errors.As(err, &got) || got.Code != code || !strings.Contains(got.Message, message) {
			t.Fatalf("fault = %v, want %s: %s", err, code, message)
		}
	}

	t.Run("arguments", func(t *testing.T) {
		for _, test := range []struct {
			action    protocol.ExternalBrowserAction
			arguments string
			message   string
		}{
			{protocol.ExternalBrowserActionClick, `"#submit"`, "browser arguments must be an object of at most 64 KiB"},
			{protocol.ExternalBrowserActionClick, `{"selector":`, "browser arguments must be an object of at most 64 KiB"},
			{protocol.ExternalBrowserActionType, `{"text":"` + strings.Repeat("a", argumentBytes) + `"}`, "browser arguments must be an object of at most 64 KiB"},
			{protocol.ExternalBrowserActionNavigate, `{"url":"file:///etc/passwd"}`, "external browser URLs must use HTTP or HTTPS without credentials"},
			{protocol.ExternalBrowserActionNavigate, `{"url":"chrome://settings"}`, "external browser URLs must use HTTP or HTTPS without credentials"},
			{protocol.ExternalBrowserActionNewTab, `{"url":"https://user:secret@example.com/"}`, "external browser URLs must use HTTP or HTTPS without credentials"},
			{protocol.ExternalBrowserActionWait, `{"seconds":-1}`, "browser wait seconds must be a non-negative representable duration"},
			{protocol.ExternalBrowserActionWait, `{"seconds":1e300}`, "browser wait seconds must be a non-negative representable duration"},
		} {
			_, err := Validate(test.action, json.RawMessage(test.arguments))
			refused(t, err, protocol.ErrorCodeInvalidRequest, test.message)
		}
		for action, arguments := range map[protocol.ExternalBrowserAction]string{
			protocol.ExternalBrowserActionNavigate: `{"url":"https://example.com/a?b=c"}`,
			protocol.ExternalBrowserActionWait:     `{"seconds":0.5}`,
			protocol.ExternalBrowserActionPageInfo: `{}`,
		} {
			if _, err := Validate(action, json.RawMessage(arguments)); err != nil {
				t.Fatalf("%s %s was refused: %v", action, arguments, err)
			}
		}
	})

	t.Run("result bounds", func(t *testing.T) {
		entries := make([]any, resultEntries+5)
		for index := range entries {
			entries[index] = "entry"
		}
		remaining := resultBytes
		value, truncated := bound(map[string]any{"links": entries, "title": "Title"}, &remaining)
		if kept := value.(map[string]any)["links"].([]any); !truncated || len(kept) != resultEntries {
			t.Fatalf("long list kept %d entries, truncated %v", len(kept), truncated)
		}
		remaining = resultBytes
		value, truncated = bound(map[string]any{"html": strings.Repeat("é", resultBytes)}, &remaining)
		text := value.(map[string]any)["html"].(string)
		if !truncated || len(text) > resultBytes || len(text) < resultBytes-8 || !strings.HasSuffix(text, "é") {
			t.Fatalf("long text kept %d bytes, truncated %v", len(text), truncated)
		}
		remaining = resultBytes
		if _, truncated = bound(map[string]any{"success": true, "count": 3.0, "title": "Title"}, &remaining); truncated {
			t.Fatal("a small result was truncated")
		}
	})

	t.Run("calls that need no browser", func(t *testing.T) {
		profile := t.TempDir()
		// A Node that did not stop cleanly leaves session temporary files.
		stale := filepath.Join(profile, "tmp", "browser-stale-1")
		kept := filepath.Join(profile, "tmp", "upload-1")
		for _, directory := range []string{filepath.Join(stale, "downloads"), kept} {
			if err := os.MkdirAll(directory, 0o700); err != nil {
				t.Fatal(err)
			}
		}
		node := protocol.NodeID{7}
		browsers := New(Options{Profile: profile, Node: node})
		browsers.find = func() string { return filepath.Join(profile, "browser-not-started") }
		if _, err := os.Stat(stale); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("stale browser files survived recovery: %v", err)
		}
		if _, err := os.Stat(kept); err != nil {
			t.Fatalf("recovery removed another owner's temporary files: %v", err)
		}
		session := protocol.NewSessionID()
		call := Call{Request: protocol.NewRequestID(), Session: session, Root: t.TempDir(), Action: protocol.ExternalBrowserActionCloseSession, Arguments: json.RawMessage(`{}`)}
		output, err := browsers.Execute(t.Context(), call)
		if err != nil {
			t.Fatal(err)
		}
		var closed struct {
			Closed bool            `json:"closed"`
			Node   protocol.NodeID `json:"execution_node"`
		}
		if output.Kind != "external_browser" || json.Unmarshal(output.Data.(json.RawMessage), &closed) != nil || !closed.Closed || closed.Node != node {
			t.Fatalf("close output = %+v", output)
		}
		// Authority is checked before anything else happens, and a stopped
		// caller starts nothing.
		denied := call
		denied.Action, denied.Arguments = protocol.ExternalBrowserActionNavigate, json.RawMessage(`{"url":"https://example.com/"}`)
		denied.Check = func(context.Context) error { return fault(protocol.ErrorCodeNotConfigured, "plugin is disabled") }
		_, err = browsers.Execute(t.Context(), denied)
		refused(t, err, protocol.ErrorCodeNotConfigured, "plugin is disabled")
		stopped, cancel := context.WithCancel(t.Context())
		cancel()
		denied.Check = nil
		_, err = browsers.Execute(stopped, denied)
		refused(t, err, protocol.ErrorCodeCancelled, "external browser call cancelled")
		browsers.Close(session)
		if sessions := browsers.Sessions(); len(sessions) != 0 {
			t.Fatalf("calls without a browser left sessions %v", sessions)
		}
		if entries, _ := os.ReadDir(filepath.Join(profile, "browser", "sessions")); len(entries) != 0 {
			t.Fatalf("calls without a browser created %d session profiles", len(entries))
		}
		browsers.Shutdown()
		if browsers.Available() {
			t.Fatal("a stopped owner still offers the browser")
		}
		_, err = browsers.Execute(t.Context(), denied)
		refused(t, err, protocol.ErrorCodeCancelled, "external browser call cancelled")
	})

	t.Run("no installed browser", func(t *testing.T) {
		profile := t.TempDir()
		// A managed browser directory of the Rust application is neither used
		// nor touched.
		legacy := filepath.Join(profile, "tools", "chromium", "legacy", "chrome")
		if err := os.MkdirAll(filepath.Dir(legacy), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(legacy, []byte("browser"), 0o700); err != nil {
			t.Fatal(err)
		}
		tree := func() []string {
			var names []string
			if err := filepath.WalkDir(profile, func(name string, _ os.DirEntry, err error) error {
				names = append(names, name)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			return names
		}
		browsers := New(Options{Profile: profile, Node: protocol.NodeID{7}})
		defer browsers.Shutdown()
		browsers.find = func() string { return "" }
		before := tree()
		const message = "no supported browser is installed on the execution Node; install Google Chrome, Chromium or Microsoft Edge"
		_, err := browsers.Browser()
		refused(t, err, protocol.ErrorCodeNotConfigured, message)
		for action, arguments := range map[protocol.ExternalBrowserAction]string{
			protocol.ExternalBrowserActionNavigate:     `{"url":"https://example.com/"}`,
			protocol.ExternalBrowserActionPageInfo:     `{}`,
			protocol.ExternalBrowserActionClick:        `{"selector":"#submit"}`,
			protocol.ExternalBrowserActionDownloads:    `{}`,
			protocol.ExternalBrowserActionCloseSession: `{}`,
			protocol.ExternalBrowserActionWait:         `{"seconds":-1}`,
		} {
			_, err := browsers.Execute(t.Context(), Call{Request: protocol.NewRequestID(), Session: protocol.NewSessionID(), Root: t.TempDir(), Action: action, Arguments: json.RawMessage(arguments)})
			refused(t, err, protocol.ErrorCodeNotConfigured, message)
		}
		if after := tree(); !slices.Equal(before, after) {
			t.Fatalf("calls without an installed browser changed the profile: %v, was %v", after, before)
		}
		if content, err := os.ReadFile(legacy); err != nil || string(content) != "browser" {
			t.Fatalf("the earlier managed browser was touched: %q, %v", content, err)
		}
		if sessions := browsers.Sessions(); len(sessions) != 0 {
			t.Fatalf("calls without an installed browser left sessions %v", sessions)
		}
	})
}
