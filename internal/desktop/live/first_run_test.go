package live

import (
	"context"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// firstRunTransport records the kind of every command the desktop sends.
type firstRunTransport struct {
	link.Transport
	mu    *sync.Mutex
	kinds *[]string
}

func (t firstRunTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	name := request.Command.Kind
	if request.Plugin != nil {
		name = request.Plugin.Package.Name + ":" + name
	}
	t.mu.Lock()
	*t.kinds = append(*t.kinds, name)
	t.mu.Unlock()
	return t.Transport.Dispatch(ctx, request)
}

// firstRun drives the whole desktop against a real Node on a new profile
// without a provider. It runs as a step of TestComposerSendRouting.
func firstRun(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 2*time.Minute)
	defer cancel()
	profile := filepath.Join(t.TempDir(), "profile")
	owner, err := node.StartWithOptions(ctx, profile, node.Options{})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Stop(context.Background())
	var mu sync.Mutex
	var kinds []string
	controller := client.New(firstRunTransport{Transport: link.NewLocal(owner.ID(), owner.ID(), owner), mu: &mu, kinds: &kinds})
	if err := controller.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(ctx, controller, preferences.ForStartup(profile))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.App.NavOpen = true
	var asked atomic.Bool
	r.SetUpdate(func(func()) { asked.Store(true) })
	var view *ui.Context
	tester := ui.NewTester(func(c *ui.Context) {
		view = c
		r.App.View(c)
	}, 1280, 820)
	tester.SetLocale("en")
	text := func(key string) string { return i18n.In("en", key) }
	sent := func() []string {
		mu.Lock()
		defer mu.Unlock()
		return append([]string(nil), kinds...)
	}
	count := func(kind string) int {
		total := 0
		for _, name := range sent() {
			if name == kind {
				total++
			}
		}
		return total
	}
	// run shows the window for a while the way the display would: a frame
	// only when one was asked for. It returns the frames drawn.
	run := func(d time.Duration) int {
		frames := 0
		for end := time.Now().Add(d); time.Now().Before(end); time.Sleep(5 * time.Millisecond) {
			tester.Advance(16 * time.Millisecond)
			if asked.Swap(false) || tester.FramePending() {
				tester.Frame()
				frames++
			}
		}
		return frames
	}
	settle := func() {
		for range 4 {
			r.workers.Wait()
			tester.Frame()
		}
		run(300 * time.Millisecond)
	}
	listed := func(id string) bool {
		for _, shown := range r.App.Sidebar.Order() {
			if shown == id {
				return true
			}
		}
		return r.App.Sidebar.Data.Find(id) != nil
	}
	settle()
	if tester.HasText(text("close")) {
		if err := tester.Click(text("close")); err != nil {
			t.Fatal(err)
		}
		settle()
	}

	// The window opens on an unsaved draft: no Node session, no sidebar row.
	draft := r.App.Sidebar.Selected
	if !strings.HasPrefix(draft, "draft/") || r.App.Page != shell.ConversationPage || r.App.Data.Find(draft) == nil {
		t.Fatalf("a new profile did not open on a draft: %q", draft)
	}
	if listed(draft) || len(r.App.Sidebar.Order()) != 0 {
		t.Fatalf("the unsent draft is listed in the sidebar: %v", r.App.Sidebar.Order())
	}
	if len(controller.Snapshot().Sessions) != 0 || count("create_session") != 0 {
		t.Fatal("opening the window created a Node session")
	}

	// Idle: no frame is asked for and no command is sent.
	before := len(sent())
	if frames := run(1500 * time.Millisecond); frames > 2 {
		t.Fatalf("the idle window redrew %d times", frames)
	}
	if idle := sent()[before:]; len(idle) != 0 {
		t.Fatalf("the idle window sent commands: %v", idle)
	}

	// New session over the draft keeps that one draft and creates nothing.
	if err := tester.Click(text("conversation")); err != nil {
		t.Fatal(err)
	}
	settle()
	if r.App.Sidebar.Selected != draft || len(r.App.Data.Unassigned) != 1 || count("create_session") != 0 || len(controller.Snapshot().Sessions) != 0 {
		t.Fatalf("New session stacked a draft or created a session: selected %q, %d conversations", r.App.Sidebar.Selected, len(r.App.Data.Unassigned))
	}

	// Without a usable model Send is not offered, and input stays a draft.
	comp := r.App.Composer(draft)
	cv := r.App.Data.Find(draft)
	blocked, _ := r.conversation(view, cv, comp)
	if !blocked.SendBlocked || blocked.SelectModel == nil {
		t.Fatal("a draft without a configured model offers Send")
	}
	if err := tester.Click(text("composer")); err != nil {
		t.Fatal(err)
	}
	tester.Type("hello")
	tester.Frame()
	if comp.Draft != "hello" {
		t.Fatalf("typing did not reach the draft: %q", comp.Draft)
	}
	before = len(sent())
	_ = tester.Click(text("send"))
	settle()
	if comp.Draft != "hello" || len(sent()) != before {
		t.Fatalf("the blocked Send acted: draft %q, commands %v", comp.Draft, sent()[before:])
	}
	// The command itself, reached any other way, fails as a normal fault.
	r.enqueue(func(c *ui.Context) {
		actions := &conversationActions{r: r, c: c, cv: cv, comp: comp}
		actions.Send(composer.Message{Text: comp.Draft, Mode: comp.Mode, Permission: comp.Permission})
	})
	settle()
	if comp.Draft != "hello" || !tester.HasText(text("chat_config_unavailable")) {
		t.Fatalf("an unconfigured send did not keep the draft and toast: draft %q", comp.Draft)
	}
	if count("create_session") != 0 || count("submit_turn") != 0 || len(controller.Snapshot().Sessions) != 0 || r.App.Sidebar.Selected != draft {
		t.Fatal("an unconfigured send reached the Node")
	}
	// The Node rejects an unconfigured creation and keeps answering.
	rejected := make(chan error, 1)
	r.execute(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Config: &protocol.SessionConfig{}}}, func(_ *ui.Context, _ protocol.Output, err error) {
		rejected <- err
	})
	settle()
	select {
	case err := <-rejected:
		if err == nil || len(controller.Snapshot().Sessions) != 0 {
			t.Fatalf("the Node created a session without a model: %v", err)
		}
	default:
		t.Fatal("the Node did not answer an unconfigured creation")
	}
	second := client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := second.Start(ctx); err != nil {
		t.Fatal("the Node stopped answering after the failed send:", err)
	}
	second.Close()

	// A draft with input survives navigation; an untouched one leaves no trace.
	r.App.Navigate(shell.SettingsPage)
	settle()
	if r.App.Data.Find(draft) == nil || comp.Draft != "hello" {
		t.Fatal("navigating away dropped a draft with input")
	}
	comp.Draft = ""
	r.App.Sidebar.Selected = ""
	settle()
	if r.App.Data.Find(draft) != nil || len(r.App.Data.Unassigned) != 0 || len(controller.Snapshot().Sessions) != 0 {
		t.Fatal("an untouched draft was kept after navigating away")
	}
	if tester.FramePending() {
		settle()
	}
	before = len(sent())
	if frames := run(time.Second); frames > 2 || len(sent()) != before {
		t.Fatalf("the settings page redrew %d times and sent %v while idle", frames, sent()[before:])
	}
}
