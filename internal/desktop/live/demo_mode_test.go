package live

import (
	"context"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
)

// Demo mode over a real Node on a new profile: the settings' order, the
// switch, the demo in place of the window's own conversations and the way
// back to them, with no command sent by anything done in the demo and
// nothing on screen that names it one.
func TestDemoMode(t *testing.T) {
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
	// Tall enough for the whole settings navigation.
	tester := ui.NewTester(r.App.View, 1280, 1600)
	tester.SetLocale("en")
	text := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	sent := func() []string {
		mu.Lock()
		defer mu.Unlock()
		return slices.Clone(kinds)
	}
	settle := func() {
		for range 4 {
			r.workers.Wait()
			tester.Frame()
		}
		for end := time.Now().Add(200 * time.Millisecond); time.Now().Before(end); time.Sleep(5 * time.Millisecond) {
			tester.Advance(16 * time.Millisecond)
			if asked.Swap(false) || tester.FramePending() {
				tester.Frame()
			}
		}
		r.workers.Wait()
		tester.Frame()
	}
	click := func(name string) {
		t.Helper()
		if err := tester.Click(name); err != nil {
			t.Fatalf("click %q: %v: %q", name, err, tester.Texts())
		}
		settle()
	}
	press := func(name string) {
		t.Helper()
		info, ok := tester.AccessByRole(ui.RoleButton, name)
		if !ok {
			t.Fatalf("no %q button: %q", name, tester.Texts())
		}
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}
	// experimental shows the Experimental settings and returns the Demo mode
	// switch.
	experimental := func() ui.AccessInfo {
		t.Helper()
		r.App.Settings.Open(settings.Experimental)
		r.App.Navigate(shell.SettingsPage)
		settle()
		info, ok := tester.AccessByRole(ui.RoleSwitch, text("settings_demo_mode"))
		if !ok {
			t.Fatalf("Experimental has no Demo mode switch: %q", tester.Texts())
		}
		return info
	}
	// marked reports a text on screen that names sample data: the preview's
	// tag, its notice and its approval toast, or the mode's own name.
	marked := func() string {
		for _, key := range []string{"preview", "preview_notice", "approval_preview_result", "settings_demo_mode"} {
			if tester.HasText(text(key)) {
				return text(key)
			}
		}
		return ""
	}
	settle()
	if tester.HasText(text("close")) {
		click(text("close"))
	}

	// The window's own state: an unsent draft with text in it.
	draft := r.App.Sidebar.Selected
	if !strings.HasPrefix(draft, "draft/") || r.App.Demo() {
		t.Fatalf("a new profile opened on %q, demo %v", draft, r.App.Demo())
	}
	click(text("composer"))
	tester.Type("keep this draft")
	settle()
	if got := r.App.Composer(draft).Draft; got != "keep this draft" {
		t.Fatalf("typing did not reach the draft: %q", got)
	}

	t.Run("settings", func(t *testing.T) {
		// About ends the navigation and Experimental comes before it.
		system := settings.Groups[len(settings.Groups)-1].Items
		if n := len(system); system[n-1].Section != settings.About || system[n-2].Section != settings.Experimental {
			t.Fatalf("the last group ends with %q, %q", system[n-2].Section, system[n-1].Section)
		}
		toggle := experimental()
		if toggle.Checked {
			t.Fatal("Demo mode is on in a new profile")
		}
		about, ok := tester.AccessByRole(ui.RoleButton, text("settings_about"))
		entry, listed := tester.AccessByRole(ui.RoleButton, text("settings_experimental"))
		if !ok || !listed {
			t.Fatalf("the navigation lacks About (%v) or Experimental (%v): %q", ok, listed, tester.Texts())
		}
		for _, info := range tester.AccessAll() {
			if info.Role != ui.RoleButton || info.Bounds.X != about.Bounds.X || info.Bounds.W != about.Bounds.W || info.Name == about.Name {
				continue
			}
			if info.Bounds.Y > about.Bounds.Y {
				t.Errorf("%q comes after About in the navigation", info.Name)
			}
			if info.Bounds.Y > entry.Bounds.Y && info.Bounds.Y < about.Bounds.Y {
				t.Errorf("%q comes between Experimental and About", info.Name)
			}
		}
		// The performance indicator moved here from General.
		if _, ok := tester.AccessByRole(ui.RoleSwitch, text("settings_sidebar_metrics")); !ok {
			t.Errorf("Experimental has no performance indicator switch: %q", tester.Texts())
		}
		r.App.Settings.Open(settings.General)
		settle()
		if _, ok := tester.AccessByRole(ui.RoleSwitch, text("settings_sidebar_metrics")); ok {
			t.Error("General still has the performance indicator switch")
		}
		if _, ok := tester.AccessByRole(ui.RoleSwitch, text("settings_demo_mode")); ok {
			t.Error("General has the Demo mode switch")
		}
	})

	t.Run("demo", func(t *testing.T) {
		experimental()
		click(text("settings_demo_mode"))
		if !r.App.Settings.DemoMode || !r.App.Demo() || !r.Preferences.Data.DemoMode {
			t.Fatalf("the switch: setting %v, window %v, preference %v", r.App.Settings.DemoMode, r.App.Demo(), r.Preferences.Data.DemoMode)
		}
		// Settings stay the profile's own: its providers do not list the
		// demo's.
		r.App.Settings.Open(settings.Providers)
		settle()
		if tester.HasText(preview.DemoModel) || !tester.HasText(text("provider_empty_live")) {
			t.Errorf("the providers' settings list the demo's provider: %q", tester.Texts())
		}
		r.App.Navigate(shell.ConversationPage)
		settle()

		// The sidebar and the transcript are the demo's, opened on its first
		// conversation, in the place of the window's own.
		demo := r.App.Data
		cv := demo.Find(preview.DemoStart)
		if cv == nil || r.App.Sidebar.Selected != preview.DemoStart || demo.Find(draft) != nil {
			t.Fatalf("the demo is not on the window: selected %q", r.App.Sidebar.Selected)
		}
		for _, want := range []string{cv.Title, "harbor-api", "lumen-web", demo.Find("lumen-web/metrics").Title, cv.Turns[len(cv.Turns)-1].User} {
			if !tester.HasText(want) {
				t.Errorf("the demo does not show %q: %q", want, tester.Texts())
			}
		}
		// The model is the demo's wherever the composer names it.
		model := text("composer_model_settings") + ": " + preview.DemoModel
		if preview.DemoModel != "DeepSeek V4.1 Pro" || !slices.ContainsFunc(tester.AccessAll(), func(info ui.AccessInfo) bool {
			return info.Role == ui.RoleButton && strings.HasPrefix(info.Name, model)
		}) {
			t.Errorf("the composer's model button does not read %q", model)
		}
		if send, ok := tester.AccessByRole(ui.RoleButton, text("composer_select_model")); ok {
			t.Errorf("the demo's composer has no model: %+v", send)
		}
		if shown := marked(); shown != "" {
			t.Errorf("the demo is named on screen: %q", shown)
		}
		// The window state saved meanwhile is the window's own.
		if saved := r.App.Prefs(); saved.PanelOf == preview.DemoStart || len(saved.Panels) != 0 && saved.Panels[preview.DemoStart].Tabs != nil {
			t.Errorf("the demo's panel is saved: %+v", saved)
		}

		// What is done in the demo changes its memory and reaches no Node:
		// an approval, then a message and the reply it plays.
		before := len(sent())
		approvals := len(cv.Waiting.Approvals)
		press(text("approval_live_count", "count", approvals))
		press(text("approval_allow_once"))
		if len(cv.Waiting.Approvals) != approvals-1 {
			t.Fatalf("the decision left %d of %d approvals", len(cv.Waiting.Approvals), approvals)
		}
		if shown := marked(); shown != "" {
			t.Errorf("the decision is named a sample: %q", shown)
		}
		tester.Key(0, ui.KeyEscape)
		settle()
		idle := demo.Find("harbor-api/flaky-test")
		click(idle.Title)
		if r.App.Sidebar.Selected != idle.ID {
			t.Fatalf("selected %q", r.App.Sidebar.Selected)
		}
		turns := len(idle.Turns)
		click(text("composer"))
		tester.Type("Run it once more")
		tester.Key(0, ui.KeyEnter)
		settle()
		if len(idle.Turns) != turns+1 || idle.Turns[turns].User != "Run it once more" {
			t.Fatalf("the message was not sent: %d turns", len(idle.Turns))
		}
		for range 600 {
			if !idle.Playing() {
				break
			}
			tester.Advance(preview.Tick)
			tester.Frame()
		}
		reply := idle.Turns[turns].Reply
		if reply.Status != conversation.Completed || len(reply.Blocks) == 0 {
			t.Fatalf("the demo's reply ended with status %d and %d blocks", reply.Status, len(reply.Blocks))
		}
		settle()
		if shown := marked(); shown != "" {
			t.Errorf("the reply is named a sample: %q", shown)
		}
		if acted := sent()[before:]; len(acted) != 0 {
			t.Errorf("the demo sent commands to the Node: %q", acted)
		}
		if len(controller.Snapshot().Sessions) != 0 {
			t.Errorf("the demo created %d Node sessions", len(controller.Snapshot().Sessions))
		}
	})

	t.Run("back", func(t *testing.T) {
		toggle := experimental()
		if !toggle.Checked {
			t.Fatal("the switch is off while the demo shows")
		}
		click(text("settings_demo_mode"))
		if r.App.Settings.DemoMode || r.App.Demo() || r.Preferences.Data.DemoMode {
			t.Fatalf("the switch: setting %v, window %v, preference %v", r.App.Settings.DemoMode, r.App.Demo(), r.Preferences.Data.DemoMode)
		}
		r.App.Navigate(shell.ConversationPage)
		settle()
		// The window's own conversations are as they were left.
		if r.App.Sidebar.Selected != draft || r.App.Data.Find(draft) == nil || r.App.Data.Find(preview.DemoStart) != nil {
			t.Fatalf("the window came back on %q", r.App.Sidebar.Selected)
		}
		if got := r.App.Composer(draft).Draft; got != "keep this draft" {
			t.Errorf("the draft came back as %q", got)
		}
		if tester.HasText("harbor-api") || len(r.App.Sidebar.Order()) != 0 {
			t.Errorf("the demo stayed in the sidebar: %v", r.App.Sidebar.Order())
		}
		if _, ok := tester.AccessByRole(ui.RoleButton, text("composer_select_model")); !ok {
			t.Errorf("the profile without a provider offers a model: %q", tester.Texts())
		}
		// The demo starts over the next time.
		experimental()
		click(text("settings_demo_mode"))
		if again := r.App.Data.Find(preview.DemoStart); again == nil || len(again.Waiting.Approvals) != 4 {
			t.Fatal("the demo did not start over")
		}
		// In Chinese it is written again in Chinese.
		english := r.App.Data.Find(preview.DemoStart).Title
		tester.SetLocale("zh-CN")
		r.App.Navigate(shell.ConversationPage)
		settle()
		if chinese := r.App.Data.Find(preview.DemoStart); chinese == nil || chinese.Title == english || !tester.HasText(chinese.Title) {
			t.Errorf("the demo in Chinese is titled %q", chinese.Title)
		}
		for _, key := range []string{"preview", "preview_notice"} {
			if tester.HasText(i18n.In("zh-CN", key)) {
				t.Errorf("the demo is named on screen: %q", i18n.In("zh-CN", key))
			}
		}
		if len(controller.Snapshot().Sessions) != 0 {
			t.Errorf("the demo created %d Node sessions", len(controller.Snapshot().Sessions))
		}
	})
}
