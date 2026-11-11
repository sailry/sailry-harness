package live

import (
	"context"
	"fmt"
	"math"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// packageCount writes a count as the statistics package does (format.js
// compact): from a thousand with one decimal and a k.
func packageCount(n int) string {
	if n < 1000 {
		return strconv.Itoa(n)
	}
	return fmt.Sprintf("%.1fk", float64(n)/1000)
}

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
	// flip presses the Demo mode switch itself.
	flip := func() {
		t.Helper()
		info := experimental()
		tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		settle()
	}
	// marked reports a text, an accessible name, value or description (a
	// tooltip) or a placeholder on screen that names sample data, in either
	// language: the preview's tag, its notice and its approval toast, the
	// wording the preview's hints share, or the mode's own name.
	fragments := []string{"ui preview", "ui 预览", "sample data", "示例数据", "preview only", "(preview)", "no runtime data", "示例", "（预览）"}
	for _, locale := range []string{"en", "zh-CN"} {
		for _, key := range []string{"preview", "preview_notice", "approval_preview_result", "settings_demo_mode", "composer_compression_hint",
			"workspace_memory_only", "workspace_terminal_preview", "resource_browser_preview", "project_editor_preview", "directory_preview", "model_score_preview"} {
			fragments = append(fragments, strings.ToLower(i18n.In(locale, key)))
		}
	}
	marked := func() string {
		shown := tester.Texts()
		for _, info := range tester.AccessAll() {
			shown = append(shown, info.Name, info.Value, info.Description, info.Placeholder)
		}
		for _, s := range shown {
			lower := strings.ToLower(s)
			for _, fragment := range fragments {
				if strings.Contains(lower, fragment) {
					return s
				}
			}
		}
		return ""
	}
	// contextPanel is the statistics package's control among the header's
	// buttons, named as its manifest labels it; contextPanelSection is the
	// heading of the usage its panel lists.
	contextPanelSection := map[string]string{"en": "Session usage", "zh-CN": "会话用量"}
	contextPanel := func(locale string) (ui.AccessInfo, bool) {
		name := map[string]string{"en": "Context", "zh-CN": "上下文"}[locale]
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == name && info.Bounds.Y+info.Bounds.H <= shell.HeaderHeight {
				return info, true
			}
		}
		return ui.AccessInfo{}, false
	}
	// tour opens every demo conversation in the window's language and, on
	// each, the mode menu, the permission menu, the model picker, the session
	// overview, the statistics package's panel and figures and the panel
	// tabs the demo offers, and fails on anything that names sample data.
	tour := func(t *testing.T, locale string) {
		in := func(key string) string { return i18n.In(locale, key) }
		// A frame of the whole window is costly to draw in software and the
		// running conversations ask for one each tick, so the tour draws
		// few, in a window no taller than it needs, and lets time pass only
		// where something has to open.
		step := func() {
			r.workers.Wait()
			tester.Frame()
		}
		// The theme's animated effects change no text.
		effects := r.App.Effects
		r.App.Effects = false
		tester.SetSize(1280, 800)
		defer func() {
			r.App.Effects = effects
			tester.SetSize(1280, 1600)
		}()
		check := func(id, where string) {
			t.Helper()
			if shown := marked(); shown != "" {
				t.Errorf("%s, %s: the demo is named on screen: %q", id, where, shown)
			}
		}
		shows := func(s string) bool { return tester.HasText(s) || slices.Contains(tester.Texts(), s) }
		// await lets time pass until expect shows ("" for whatever opens)
		// and checks what did.
		await := func(id, where, expect string) {
			t.Helper()
			step()
			for range 8 {
				if expect == "" || shows(expect) {
					break
				}
				tester.Advance(50 * time.Millisecond)
				step()
			}
			if expect != "" && !shows(expect) {
				t.Errorf("%s: the %s does not show %q: %q", id, where, expect, tester.Texts())
			}
			check(id, where)
		}
		// open presses the first button found by name, checks what it opened
		// and closes it. then names a button of what opened to press as
		// well, and the text that shows.
		open := func(id, where, expect string, match func(name string) bool, then ...string) bool {
			t.Helper()
			for _, info := range tester.AccessAll() {
				if info.Role != ui.RoleButton && info.Role != ui.RoleMenuButton && info.Role != ui.RolePopUpButton || !match(info.Name) {
					continue
				}
				tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				await(id, where, expect)
				if len(then) == 2 {
					if inner, ok := tester.AccessByRole(ui.RoleButton, then[0]); ok {
						tester.ClickAt(inner.Bounds.X+inner.Bounds.W/2, inner.Bounds.Y+inner.Bounds.H/2)
						await(id, where+" list", then[1])
					} else {
						t.Errorf("%s: the %s has no %q button", id, where, then[0])
					}
				}
				tester.Key(0, ui.KeyEscape)
				step()
				return true
			}
			return false
		}
		named := func(keys ...string) func(string) bool {
			return func(name string) bool {
				return slices.ContainsFunc(keys, func(key string) bool { return name == in(key) })
			}
		}
		// until draws frames until cond holds: a package's view is mounted
		// and publishes over a few.
		until := func(cond func() bool) bool {
			for range 40 {
				if cond() {
					return true
				}
				tester.Advance(16 * time.Millisecond)
				step()
			}
			return cond()
		}
		demo := r.App.Data
		list := slices.Clone(demo.Unassigned)
		for _, pr := range demo.Projects {
			list = append(list, pr.Conversations...)
		}
		if len(list) < 11 {
			t.Fatalf("the demo has %d conversations", len(list))
		}
		r.App.Navigate(shell.ConversationPage)
		step()
		for _, cv := range list {
			// A row past its group's first five is behind Show more.
			if tester.Click(cv.Title) != nil || r.App.Sidebar.Selected != cv.ID {
				r.App.Sidebar.Selected = cv.ID
			}
			step()
			if !tester.HasText(cv.Title) {
				t.Errorf("%s is not shown: %q", cv.ID, tester.Texts())
			}
			check(cv.ID, "transcript")
			composed := !cv.Session.ReadOnly
			if got := open(cv.ID, "mode menu", in("composer_mode_plan_description"), named("composer_mode_code", "composer_mode_plan", "composer_mode_flagship")); got != composed {
				t.Errorf("%s: mode menu %v, composer %v", cv.ID, got, composed)
			}
			if got := open(cv.ID, "permission menu", in("composer_permission_full_description"), named("composer_permission_ask", "composer_permission_project", "composer_permission_full")); got != composed {
				t.Errorf("%s: permission menu %v, composer %v", cv.ID, got, composed)
			}
			if got := open(cv.ID, "model picker", in("composer_effort"), func(name string) bool {
				return strings.HasPrefix(name, in("composer_model_settings")+": "+preview.DemoModel)
			}, preview.DemoModel, "DeepSeek V4.1"); got != composed {
				t.Errorf("%s: model picker %v, composer %v", cv.ID, got, composed)
			}
			if !open(cv.ID, "overview", "", named("chat_assets_title")) {
				t.Errorf("%s: no session overview button", cv.ID)
			}
			// The statistics package shows over a demo conversation as over
			// a session: its context panel in the header and, beside a
			// composer, its figures of the conversation's own usage, the
			// speed to one decimal as the package writes it. The first
			// conversation's panel and a figure's card are opened.
			if !until(func() bool { _, ok := contextPanel(locale); return ok }) {
				t.Errorf("%s: the header has no context panel button: %q", cv.ID, tester.Texts())
			} else if cv == list[0] {
				at, _ := contextPanel(locale)
				tester.ClickAt(at.Bounds.X+at.Bounds.W/2, at.Bounds.Y+at.Bounds.H/2)
				await(cv.ID, "context panel", contextPanelSection[locale])
				if opened, _ := contextPanel(locale); !opened.Expanded || opened.Disabled {
					t.Errorf("%s: the context panel button is expanded %v, disabled %v", cv.ID, opened.Expanded, opened.Disabled)
				}
				tester.Key(0, ui.KeyEscape)
				step()
			}
			if usage := cv.Session.Usage; composed && usage != nil {
				speed := fmt.Sprintf("%.1f/s", usage.Speed)
				if !until(func() bool { return shows(packageCount(usage.Tokens)) && shows(speed) }) {
					t.Errorf("%s: the context bar lacks the figures %s and %s: %q", cv.ID, packageCount(usage.Tokens), speed, tester.Texts())
				} else if cv == list[0] {
					at, _ := tester.Find(speed)
					tester.Move(at.X+at.W/2, at.Y+at.H/2)
					tester.Advance(time.Second)
					await(cv.ID, "speed card", packageCount(usage.Tokens/demoOutputShare))
					tester.Move(0, 0)
					step()
				}
			}
			// The context ring reads the tokens used of the model's window,
			// as a measured indicator does. Its tooltip is the same text:
			// the first conversation's is hovered open.
			if usage := cv.Session.Usage; composed && usage != nil {
				window := conversation.Count(usage.Window)
				ring, ok := ui.AccessInfo{}, false
				for _, info := range tester.AccessAll() {
					if strings.Contains(info.Name, " / "+window+" ") && strings.HasSuffix(info.Name, "%") {
						ring, ok = info, true
						break
					}
				}
				if !ok {
					t.Errorf("%s: no context ring reads its use of %s", cv.ID, window)
				} else if cv == list[0] {
					tester.Move(ring.Bounds.X+ring.Bounds.W/2, ring.Bounds.Y+ring.Bounds.H/2)
					tester.Advance(time.Second)
					step()
					check(cv.ID, "context ring tooltip")
					tester.Move(0, 0)
					step()
				}
			}
			r.App.PanelOpen = true
			for _, kind := range []panel.Kind{panel.Files, panel.Changes, panel.History, panel.Documents} {
				r.App.Panel.Open(kind)
				step()
				check(cv.ID, "panel "+string(kind))
			}
			r.App.PanelOpen = false
			step()
		}
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

	// A sample usage as the statistics a session records, which the
	// statistics package reads over a demo conversation.
	t.Run("statistics", func(t *testing.T) {
		if demoStatistics(nil) != nil {
			t.Error("a conversation without usage has statistics")
		}
		got := demoStatistics(&conversation.Usage{Tokens: 48200, Speed: 71.4, Cost: 0.0312, Cached: 36400, Turns: 2, Context: 0.24, Window: 200000})
		if got.Turns != 2 || got.Responses != 2 || got.Usage == nil || got.Usage.Input != 42175 || got.Usage.Output != 6025 ||
			got.Usage.CachedInput != 36400 || got.Observations.CacheRead == nil || *got.Observations.CacheRead != 36400 {
			t.Errorf("the tokens are recorded as %+v, %+v", got, got.Usage)
		}
		if got.Cost == nil || got.Cost.UsdMicros != 31200 || got.Cost.Responses != 2 {
			t.Errorf("the cost is recorded as %+v", got.Cost)
		}
		if generation := got.Generation; generation == nil || generation.OutputTokens != 6025 || generation.Responses != 2 ||
			math.Abs(float64(generation.OutputTokens)*1_000_000/float64(generation.ElapsedUs)-71.4) > 0.001 {
			t.Errorf("the speed is recorded as %+v", generation)
		}
		if got.ContextTokens == nil || *got.ContextTokens != 48000 || got.ContextLimit == nil || *got.ContextLimit != 200000 {
			t.Errorf("the context is recorded as %v of %v", got.ContextTokens, got.ContextLimit)
		}
		// More cached than input is all of the input; without a speed or a
		// window neither is recorded.
		bare := demoStatistics(&conversation.Usage{Tokens: 800, Cached: 5000})
		if bare.Usage.Input != 700 || bare.Usage.CachedInput != 700 || bare.Generation != nil || bare.ContextTokens != nil || bare.ContextLimit != nil {
			t.Errorf("a bare usage is recorded as %+v, %+v", bare, bare.Usage)
		}
		body, err := demoComposer(&viewmodel.Conversation{ID: "sample", Session: conversation.Session{Usage: &conversation.Usage{Tokens: 800, Window: 1000}}}, true)
		if err != nil || body["session"] != nil || body["connected"] != true || body["has_messages"] != false || body["sidebar"] != true || body["statistics"] == nil {
			t.Errorf("the composer state of a demo conversation is %v, %v", body, err)
		}
	})

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
		flip()
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
		visited := len(sent())
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
		// The context bar's figures are the statistics package's, of the
		// conversation's own usage: tokens, speed, cost, cache rate and
		// turns, each with its rows on hover. The header has the package's
		// context panel, which opens on the same usage. Nothing of it was
		// asked of the Node: the package's view is the one the draft's
		// composer already mounted.
		usage := cv.Session.Usage
		input, cents := usage.Tokens-usage.Tokens/demoOutputShare, int(math.Round(usage.Cost*1_000_000))/10_000
		figures := []string{packageCount(usage.Tokens), fmt.Sprintf("%.1f/s", usage.Speed), fmt.Sprintf("%d.%02d", cents/100, cents%100),
			fmt.Sprintf("%.0f%%", 100*float64(usage.Cached)/float64(input))}
		for range 20 {
			if tester.HasText(figures[1]) {
				break
			}
			settle()
		}
		for _, figure := range figures {
			if !tester.HasText(figure) {
				t.Fatalf("the context bar lacks the figure %s of %q: %q", figure, figures, tester.Texts())
			}
		}
		hover := func(figure string, rows ...string) {
			t.Helper()
			at, _ := tester.Find(figure)
			tester.Move(at.X+at.W/2, at.Y+at.H/2)
			tester.Advance(time.Second)
			settle()
			for _, row := range rows {
				if !tester.HasText(row) {
					t.Errorf("the card of %s lacks %q: %q", figure, row, tester.Texts())
				}
			}
			if shown := marked(); shown != "" {
				t.Errorf("the card of %s names the demo: %q", figure, shown)
			}
			tester.Move(0, 0)
			tester.Advance(time.Second)
			settle()
		}
		hover(figures[0], "Tokens", "This session", "Subagents")
		hover(figures[1], "Session average", "Timed responses", "Timed output", packageCount(usage.Tokens/demoOutputShare), "TTFT")
		hover(figures[2], "Cost", "This session", "$"+figures[2], "Subagents")
		hover(figures[3], "Cache", "Cached input", packageCount(usage.Cached), "Uncached input", "Input tokens", packageCount(input))
		// The turns, a bare number, are read where they are published.
		published := r.demoPluginRegistry(cv).Entries(protocol.PluginUiSlotStatistics)
		if len(published) != 5 || string(published[4].State.Value) != strconv.Quote(fmt.Sprint(usage.Turns)) ||
			len(published[4].State.Details) != 1 || published[4].State.Details[0].Value != fmt.Sprint(usage.Turns) {
			t.Errorf("the statistics slot over the demo publishes %+v", published)
		}
		panel, ok := contextPanel("en")
		if !ok || panel.Disabled || panel.Expanded {
			t.Fatalf("the header's context panel button: found %v, %+v", ok, panel)
		}
		tester.ClickAt(panel.Bounds.X+panel.Bounds.W/2, panel.Bounds.Y+panel.Bounds.H/2)
		tester.Advance(time.Second)
		settle()
		if opened, _ := contextPanel("en"); !opened.Expanded || opened.Disabled || !tester.HasText(contextPanelSection["en"]) {
			t.Errorf("the context panel: expanded %v, disabled %v, texts %q", opened.Expanded, opened.Disabled, tester.Texts())
		}
		if shown := marked(); shown != "" {
			t.Errorf("the context panel names the demo: %q", shown)
		}
		tester.Key(0, ui.KeyEscape)
		settle()
		if closed, _ := contextPanel("en"); closed.Expanded {
			t.Error("Escape left the context panel open")
		}
		for _, kind := range sent()[visited:] {
			if kind == "read_plugin_view" || strings.HasPrefix(kind, pluginStatisticsPackage+":") {
				t.Errorf("showing the statistics package over the demo sent %q to the Node", kind)
			}
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
		t.Run("everywhere", func(t *testing.T) { tour(t, "en") })
		if acted := sent()[before:]; len(acted) != 0 {
			t.Errorf("the tour sent commands to the Node: %q", acted)
		}
	})

	t.Run("back", func(t *testing.T) {
		toggle := experimental()
		if !toggle.Checked {
			t.Fatal("the switch is off while the demo shows")
		}
		flip()
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
		// What the statistics package mounted over the demo left with it.
		for _, owner := range r.pluginViews.registries {
			if strings.HasPrefix(owner.draft, pluginDemoPresentation) {
				t.Errorf("the contributions over %q outlived the demo", owner.draft)
			}
		}
		for key := range r.pluginViews.mounts {
			if strings.HasPrefix(key.scope.presentation, pluginDemoPresentation) {
				t.Errorf("the view mounted over %q outlived the demo", key.scope.presentation)
			}
		}
		if _, ok := tester.AccessByRole(ui.RoleButton, text("composer_select_model")); !ok {
			t.Errorf("the profile without a provider offers a model: %q", tester.Texts())
		}
		// The demo starts over the next time.
		flip()
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
		if shown := marked(); shown != "" {
			t.Errorf("the demo is named on screen: %q", shown)
		}
		before := len(sent())
		t.Run("everywhere", func(t *testing.T) { tour(t, "zh-CN") })
		if acted := sent()[before:]; len(acted) != 0 {
			t.Errorf("the tour sent commands to the Node: %q", acted)
		}
		if len(controller.Snapshot().Sessions) != 0 {
			t.Errorf("the demo created %d Node sessions", len(controller.Snapshot().Sessions))
		}
	})
}
