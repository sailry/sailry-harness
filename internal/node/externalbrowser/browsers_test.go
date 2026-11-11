package externalbrowser

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/chromedp/cdproto/target"
	"github.com/jonboulle/clockwork"
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

// lifetimeFixture is an owner whose browsers are stand-ins: a context, a
// profile directory and a temporary directory, with no process behind them.
// Everything about when a browser starts, stays and goes runs for real.
type lifetimeFixture struct {
	t        *testing.T
	browsers *Browsers
	clock    *clockwork.FakeClock
	profile  string

	mu       sync.Mutex
	launched []protocol.SessionID
	gate     chan struct{} // when set, a launch waits for it
	entered  chan protocol.SessionID
}

func newLifetimeFixture(t *testing.T) *lifetimeFixture {
	t.Helper()
	fixture := &lifetimeFixture{t: t, clock: clockwork.NewFakeClock(), profile: t.TempDir(), entered: make(chan protocol.SessionID, 16)}
	fixture.browsers = New(Options{Profile: fixture.profile, Node: protocol.NodeID{7}, Clock: fixture.clock})
	fixture.browsers.find = func() string { return filepath.Join(fixture.profile, "browser-not-started") }
	fixture.browsers.launch = func(ctx context.Context, id protocol.SessionID, _ string) (*session, error) {
		fixture.mu.Lock()
		fixture.launched = append(fixture.launched, id)
		gate := fixture.gate
		fixture.mu.Unlock()
		fixture.entered <- id
		if gate != nil {
			<-gate
		}
		profile := filepath.Join(fixture.browsers.sessionsRoot(), id.String(), "profile")
		_, absent := os.Stat(profile)
		if err := os.MkdirAll(profile, 0o700); err != nil {
			return nil, err
		}
		temporary, err := os.MkdirTemp(t.TempDir(), "browser-")
		if err != nil {
			return nil, err
		}
		running, cancel := context.WithCancel(fixture.browsers.processes)
		return &session{ctx: running, cancel: cancel, profile: profile, temporary: temporary, windows: map[target.ID]*window{},
			clock: fixture.clock, resumed: absent == nil}, nil
	}
	t.Cleanup(fixture.browsers.Shutdown)
	return fixture
}

// wait runs a plain wait in the session's browser and returns its output.
func (fixture *lifetimeFixture) wait(id protocol.SessionID, seconds float64) (map[string]any, error) {
	arguments, _ := json.Marshal(map[string]any{"seconds": seconds})
	output, err := fixture.browsers.Execute(fixture.t.Context(), Call{Request: protocol.NewRequestID(), Session: id, Root: fixture.profile, Action: protocol.ExternalBrowserActionWait, Arguments: arguments})
	if err != nil {
		return nil, err
	}
	var value map[string]any
	if err := json.Unmarshal(output.Data.(json.RawMessage), &value); err != nil {
		fixture.t.Fatal(err)
	}
	return value, nil
}

func (fixture *lifetimeFixture) use(id protocol.SessionID) map[string]any {
	fixture.t.Helper()
	value, err := fixture.wait(id, 0)
	if err != nil {
		fixture.t.Fatal(err)
	}
	return value
}

func (fixture *lifetimeFixture) launches() int {
	fixture.mu.Lock()
	defer fixture.mu.Unlock()
	return len(fixture.launched)
}

// eventually waits for a state that a timer callback or another goroutine
// reaches on its own.
func (fixture *lifetimeFixture) eventually(what string, reached func() bool) {
	fixture.t.Helper()
	for deadline := time.Now().Add(10 * time.Second); !reached(); time.Sleep(2 * time.Millisecond) {
		if time.Now().After(deadline) {
			fixture.t.Fatalf("%s did not happen", what)
		}
	}
}

// busy reports how many sessions have a call under way.
func (fixture *lifetimeFixture) busy() int {
	fixture.browsers.mu.Lock()
	defer fixture.browsers.mu.Unlock()
	count := 0
	for _, slot := range fixture.browsers.sessions {
		if slot.users > 0 && slot.session != nil {
			count++
		}
	}
	return count
}

// finish lets the waits under way run out and returns when their calls end.
func (fixture *lifetimeFixture) finish(calls *sync.WaitGroup) {
	done := make(chan struct{})
	go func() {
		calls.Wait()
		close(done)
	}()
	for {
		select {
		case <-done:
			return
		case <-time.After(2 * time.Millisecond):
			fixture.clock.Advance(time.Second)
		}
	}
}

func (fixture *lifetimeFixture) slots() int {
	fixture.browsers.mu.Lock()
	defer fixture.browsers.mu.Unlock()
	return len(fixture.browsers.sessions)
}

// TestBrowserLifetime covers when a session's browser starts, how long it
// stays and what closing leaves behind. No browser process is launched.
func TestBrowserLifetime(t *testing.T) {
	refused := func(t *testing.T, err error, code protocol.ErrorCode) {
		t.Helper()
		var got *protocol.Fault
		if !errors.As(err, &got) || got.Code != code {
			t.Fatalf("fault = %v, want %s", err, code)
		}
	}
	exists := func(path string) bool {
		_, err := os.Stat(path)
		return err == nil
	}

	t.Run("an idle browser closes and keeps its profile", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		conversation := protocol.NewSessionID()
		if first := fixture.use(conversation); first["notice"] != nil {
			t.Fatalf("the first browser of a conversation reported %v", first["notice"])
		}
		profile := filepath.Join(fixture.browsers.sessionsRoot(), conversation.String(), "profile")
		// A call inside the idle period keeps the browser and restarts the period.
		fixture.clock.Advance(idleTimeout - time.Second)
		fixture.use(conversation)
		fixture.clock.Advance(idleTimeout - time.Second)
		if sessions := fixture.browsers.Sessions(); len(sessions) != 1 || fixture.launches() != 1 {
			t.Fatalf("a used browser was closed: sessions %v, launches %d", sessions, fixture.launches())
		}
		fixture.clock.Advance(time.Second)
		fixture.eventually("the idle close", func() bool { return len(fixture.browsers.Sessions()) == 0 && fixture.slots() == 0 })
		if !exists(profile) {
			t.Fatal("an idle close removed the conversation's profile")
		}
		// The next call starts a browser again and says so.
		again := fixture.use(conversation)
		if notice, _ := again["notice"].(string); !strings.Contains(notice, "a new browser was started") || fixture.launches() != 2 {
			t.Fatalf("restart notice = %v after %d launches", again["notice"], fixture.launches())
		}
		if later := fixture.use(conversation); later["notice"] != nil {
			t.Fatalf("a running browser reported %v", later["notice"])
		}
	})

	t.Run("the longest idle browser gives up its place at the limit", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		var conversations []protocol.SessionID
		for range maxSessions {
			conversation := protocol.NewSessionID()
			conversations = append(conversations, conversation)
			fixture.use(conversation)
			fixture.clock.Advance(time.Second)
		}
		// The first conversation is used again, so the second is now the
		// one idle longest.
		fixture.use(conversations[0])
		fixture.clock.Advance(time.Second)
		fifth := protocol.NewSessionID()
		fixture.use(fifth)
		running := fixture.browsers.Sessions()
		if len(running) != maxSessions || slices.Contains(running, conversations[1]) || !slices.Contains(running, conversations[0]) || !slices.Contains(running, fifth) {
			t.Fatalf("running sessions = %v, want the fifth instead of %v", running, conversations[1])
		}
		if !exists(filepath.Join(fixture.browsers.sessionsRoot(), conversations[1].String(), "profile")) {
			t.Fatal("giving up a place removed the conversation's profile")
		}
		if fixture.slots() != maxSessions {
			t.Fatalf("%d slots are kept for %d browsers", fixture.slots(), maxSessions)
		}

		// With a call under way in every browser none can give up its
		// place, and only then is the answer busy.
		var calls sync.WaitGroup
		for _, conversation := range running {
			calls.Go(func() {
				if _, err := fixture.wait(conversation, 20); err != nil {
					t.Errorf("wait in a busy browser: %v", err)
				}
			})
		}
		fixture.eventually("four calls under way", func() bool { return fixture.busy() == maxSessions })
		_, err := fixture.wait(protocol.NewSessionID(), 0)
		refused(t, err, protocol.ErrorCodeBusy)
		if fixture.slots() != maxSessions {
			t.Fatalf("a refused session left a slot: %d", fixture.slots())
		}
		fixture.finish(&calls)
	})

	t.Run("two sessions never take the last place together", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		var calls sync.WaitGroup
		for range maxSessions - 1 {
			conversation := protocol.NewSessionID()
			fixture.use(conversation)
			calls.Go(func() {
				if _, err := fixture.wait(conversation, 20); err != nil {
					t.Errorf("wait in a busy browser: %v", err)
				}
			})
		}
		fixture.eventually("three calls under way", func() bool { return fixture.busy() == maxSessions-1 })
		for len(fixture.entered) > 0 {
			<-fixture.entered
		}
		// The fourth browser is still starting when a fifth session asks.
		gate := make(chan struct{})
		fixture.mu.Lock()
		fixture.gate = gate
		fixture.mu.Unlock()
		starting := protocol.NewSessionID()
		calls.Go(func() {
			if _, err := fixture.wait(starting, 0); err != nil {
				t.Errorf("the session that took the last place: %v", err)
			}
		})
		if entered := <-fixture.entered; entered != starting {
			t.Fatalf("launch of %v, want %v", entered, starting)
		}
		_, err := fixture.wait(protocol.NewSessionID(), 0)
		refused(t, err, protocol.ErrorCodeBusy)
		if fixture.launches() != maxSessions {
			t.Fatalf("%d browsers were launched for %d places", fixture.launches(), maxSessions)
		}
		close(gate)
		fixture.finish(&calls)
	})

	t.Run("a wait beyond the limit ends at the limit and keeps the browser", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		conversation := protocol.NewSessionID()
		fixture.use(conversation)
		type outcome struct {
			value map[string]any
			err   error
		}
		done := make(chan outcome, 1)
		go func() {
			value, err := fixture.wait(conversation, 600)
			done <- outcome{value, err}
		}()
		// A call under way has stopped the idle timer, so the wait is the
		// clock's only waiter.
		fixture.eventually("the wait under way", func() bool { return fixture.busy() == 1 })
		if err := fixture.clock.BlockUntilContext(t.Context(), 1); err != nil {
			t.Fatal(err)
		}
		fixture.clock.Advance(waitLimit - time.Second)
		select {
		case early := <-done:
			t.Fatalf("the wait ended before the limit: %v", early)
		case <-time.After(20 * time.Millisecond):
		}
		fixture.clock.Advance(time.Second)
		var result outcome
		select {
		case result = <-done:
		case <-time.After(10 * time.Second):
			t.Fatal("a wait of 600 seconds did not end at the limit")
		}
		waited, _ := result.value["result"].(map[string]any)
		if result.err != nil || waited["waited_seconds"] != waitLimit.Seconds() {
			t.Fatalf("limited wait = %v, %v", result.value, result.err)
		}
		if sessions := fixture.browsers.Sessions(); len(sessions) != 1 || fixture.launches() != 1 {
			t.Fatalf("a long wait cost the browser: sessions %v, launches %d", sessions, fixture.launches())
		}
		for arguments, want := range map[string]time.Duration{
			`{"timeout":60}`: waitLimit, `{"timeout":5}`: 5 * time.Second, `{}`: 30 * time.Second, `{"timeout":-3}`: 0, `{"timeout":1e18}`: waitLimit,
		} {
			var decoded map[string]any
			if err := json.Unmarshal([]byte(arguments), &decoded); err != nil {
				t.Fatal(err)
			}
			if got := waitFor(decoded, "timeout", 30); got != want {
				t.Fatalf("waitFor(%s) = %v, want %v", arguments, got, want)
			}
		}
		if waitLimit >= operationTimeout-10*time.Second {
			t.Fatalf("a wait of %v leaves no room under the call deadline of %v", waitLimit, operationTimeout)
		}
	})

	t.Run("closing keeps the profile and removing deletes it", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		conversation, other := protocol.NewSessionID(), protocol.NewSessionID()
		fixture.use(conversation)
		fixture.use(other)
		directory := filepath.Join(fixture.browsers.sessionsRoot(), conversation.String())
		fixture.browsers.Close(conversation)
		if !exists(filepath.Join(directory, "profile")) || fixture.slots() != 1 {
			t.Fatalf("close: profile kept %v, slots %d", exists(directory), fixture.slots())
		}
		// Removing works with and without a running browser.
		fixture.browsers.Remove(conversation)
		fixture.browsers.Remove(other)
		for _, removed := range []protocol.SessionID{conversation, other} {
			if exists(filepath.Join(fixture.browsers.sessionsRoot(), removed.String())) {
				t.Fatalf("the profile of removed conversation %v is still on disk", removed)
			}
		}
		if sessions, profiles := fixture.browsers.Sessions(), fixture.browsers.Profiles(); len(sessions) != 0 || len(profiles) != 0 || fixture.slots() != 0 {
			t.Fatalf("after removal: sessions %v, profiles %v, slots %d", sessions, profiles, fixture.slots())
		}
		// A profile left by a conversation deleted while no Node ran is listed.
		orphan := protocol.NewSessionID()
		if err := os.MkdirAll(filepath.Join(fixture.browsers.sessionsRoot(), orphan.String(), "profile"), 0o700); err != nil {
			t.Fatal(err)
		}
		if profiles := fixture.browsers.Profiles(); len(profiles) != 1 || profiles[0] != orphan {
			t.Fatalf("profiles on disk = %v, want %v", profiles, orphan)
		}
	})

	t.Run("staged uploads stay until the browser closes", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		conversation := protocol.NewSessionID()
		fixture.use(conversation)
		// The temporary directory is reached through a link on macOS.
		worktree, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(worktree, "report.txt"), []byte("hello"), 0o600); err != nil {
			t.Fatal(err)
		}
		// The file input is absent, so the operation fails after staging;
		// the copy the page would read later is kept either way.
		_, err = fixture.browsers.Execute(t.Context(), Call{Request: protocol.NewRequestID(), Session: conversation, Root: worktree,
			Action: protocol.ExternalBrowserActionFileUpload, Arguments: json.RawMessage(`{"selector":"#file","file_path":"report.txt"}`)})
		refused(t, err, protocol.ErrorCodeUnavailable)
		fixture.browsers.mu.Lock()
		running := fixture.browsers.sessions[conversation].session
		fixture.browsers.mu.Unlock()
		if len(running.uploads) != 1 {
			t.Fatalf("staged uploads = %v", running.uploads)
		}
		first := filepath.Join(running.uploads[0], "report.txt")
		if content, err := os.ReadFile(first); err != nil || string(content) != "hello" {
			t.Fatalf("the staged upload was not kept after its call: %q, %v", content, err)
		}
		// Only the oldest copies go once the bound is reached.
		var last string
		for range stagedUploads {
			staged, err := running.stageUpload(t.Context(), worktree, map[string]any{"file_path": "report.txt"})
			if err != nil {
				t.Fatal(err)
			}
			last = staged
		}
		if exists(first) || !exists(last) || len(running.uploads) != stagedUploads {
			t.Fatalf("bounded uploads: oldest kept %v, newest kept %v, %d directories", exists(first), exists(last), len(running.uploads))
		}
		for _, escaping := range []string{"../outside.txt", filepath.Join(filepath.Dir(worktree), "outside.txt")} {
			if _, err := running.stageUpload(t.Context(), worktree, map[string]any{"file_path": escaping}); err == nil {
				t.Fatalf("upload of %q was staged", escaping)
			}
		}
		fixture.browsers.Close(conversation)
		if exists(last) || exists(running.temporary) {
			t.Fatal("closing the browser left its staged uploads")
		}
	})

	t.Run("stopping closes every browser and starts none", func(t *testing.T) {
		fixture := newLifetimeFixture(t)
		conversation := protocol.NewSessionID()
		fixture.use(conversation)
		busy := protocol.NewSessionID()
		fixture.use(busy)
		interrupted := make(chan error, 1)
		go func() {
			_, err := fixture.wait(busy, 20)
			interrupted <- err
		}()
		fixture.eventually("a call under way", func() bool { return fixture.busy() == 1 })
		fixture.browsers.Shutdown()
		refused(t, <-interrupted, protocol.ErrorCodeCancelled)
		if sessions := fixture.browsers.Sessions(); len(sessions) != 0 || fixture.slots() != 0 || fixture.browsers.processes.Err() == nil {
			t.Fatalf("after shutdown: sessions %v, slots %d", sessions, fixture.slots())
		}
		_, err := fixture.wait(conversation, 0)
		refused(t, err, protocol.ErrorCodeCancelled)
		// An idle timer that fires after the stop finds nothing to close.
		fixture.clock.Advance(2 * idleTimeout)
		if fixture.launches() != 2 {
			t.Fatalf("%d launches", fixture.launches())
		}
	})
}
