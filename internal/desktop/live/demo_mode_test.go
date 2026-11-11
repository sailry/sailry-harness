package live

import (
	"context"
	"encoding/json"
	"fmt"
	"image/png"
	"math"
	"os"
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
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
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
	if n >= 1000000 {
		return fmt.Sprintf("%.1fM", float64(n)/1000000)
	}
	return fmt.Sprintf("%.1fk", float64(n)/1000)
}

// Demo mode over a real Node on a new profile: the settings' order, the
// switch, the demo in place of the window's own conversations and the way
// back to them, with no command sent by anything done in the demo and
// nothing on screen that names it one. The Usage page's calls answer from
// the demo's ledger meanwhile and from the Node again after the way back,
// the page's watch moving over with the switch.
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
	// shot saves the frame at 2x in the dark theme to
	// $SAILRY_SNAPSHOTS/name.png, when set.
	shot := func(name string) {
		t.Helper()
		dir := os.Getenv("SAILRY_SNAPSHOTS")
		if dir == "" {
			return
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		tester.SetDark(true)
		tester.SetScale(2)
		settle()
		file, err := os.Create(filepath.Join(dir, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		err = png.Encode(file, tester.Image())
		file.Close()
		if err != nil {
			t.Fatal(err)
		}
		tester.SetDark(false)
		tester.SetScale(1)
		settle()
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
	// The Usage page: the host a page of the statistics package is mounted
	// with, as far as usage goes, and what the page asks it. The switch
	// tells the plugin pages each time it is flipped.
	var page *desktopplugins.Host
	defer func() {
		if page != nil {
			_ = page.Close()
		}
	}()
	sdk := map[string]desktopplugins.HostOperation{}
	ask := func(name string, arguments ...string) (any, error) {
		operation := sdk[name]
		if operation == nil {
			return nil, fmt.Errorf("the SDK has no %s", name)
		}
		raw := make([]json.RawMessage, len(arguments))
		for i, argument := range arguments {
			raw[i] = json.RawMessage(argument)
		}
		body, err := operation(ctx, raw)
		if err != nil {
			return nil, err
		}
		var value any
		return value, json.Unmarshal(body, &value)
	}
	// bind takes the SDK of a page's host, as the page's script does.
	bind := func(host *desktopplugins.Host) error {
		modules, err := host.Modules()
		if err != nil {
			return err
		}
		clear(sdk)
		for _, module := range modules {
			if module.Name != "sailry/sdk" {
				continue
			}
			for _, export := range module.Exports {
				if sdk[export.Name] = export.Sync; export.Sync == nil {
					sdk[export.Name] = export.Async
				}
			}
		}
		return nil
	}
	at := func(value any, path ...string) any {
		for _, key := range path {
			object, _ := value.(map[string]any)
			value = object[key]
		}
		return value
	}
	quote := func(text string) string { return strconv.Quote(text) }
	type usageAnswer struct {
		value any
		err   error
	}
	waiting := make(chan usageAnswer, 1)
	day := time.Now().UTC().Truncate(24 * time.Hour).Add(24 * time.Hour).UnixMilli()
	month := fmt.Sprintf(`{"start_ms":"%d","end_ms":"%d","utc_offset_minutes":0,"dimension":"model","projects":[],"worktrees":[],"providers":[],"models":[],"before":null}`, day-30*86400000, day)
	changes := 0
	if changed := r.App.Hooks.DemoChanged; changed == nil {
		t.Fatal("the plugin pages do not follow the Demo mode switch")
	} else {
		r.App.Hooks.DemoChanged = func() { changes++; changed() }
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
		// The tool calls are the transcript's, by name; a question or a plan
		// review is none.
		tools := demoTools([]conversation.Turn{
			{Reply: conversation.Reply{Blocks: []conversation.Block{{Kind: conversation.ToolsBlock, Calls: []conversation.Call{
				{Name: "read"}, {Name: "read"}, {Name: "command"}, {Name: "plan_review", Terminal: true}, {}}}}}},
			{Reply: conversation.Reply{Blocks: []conversation.Block{{Kind: conversation.TextBlock}, {Kind: conversation.ToolsBlock, Calls: []conversation.Call{{Name: "read"}}}}}},
		})
		if len(tools) != 2 || tools["read"] != 3 || tools["command"] != 1 {
			t.Errorf("the transcript's tool calls are counted as %v", tools)
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
		shot("demo-figures")
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
			shot("demo-card-" + strings.ReplaceAll(strings.ToLower(rows[0]), " ", "-"))
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
		// Its tool calls are the ones the transcript shows.
		if calls := demoTools(cv.Turns); len(calls) == 0 || !tester.HasText("Tool calls") {
			t.Errorf("the context panel counts the tool calls %v: %q", calls, tester.Texts())
		}
		shot("demo-context-panel")
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

		// The Usage page reads the demo's ledger: one source, this device,
		// with figures over the demo's projects and models, and nothing
		// asked of the Node.
		t.Run("usage", func(t *testing.T) {
			if changes != 1 || r.pluginUsageSample() == nil {
				t.Fatalf("the switch told the plugin pages %d times; a sample %v", changes, r.pluginUsageSample() != nil)
			}
			reference := protocol.PluginReference{Name: "statistics"}
			for _, summary := range controller.Snapshot().Plugins {
				if summary.Name == reference.Name {
					reference = packageReference(summary)
				}
			}
			scope := protocol.DefaultPluginContext()
			scope.Package, scope.Surface.Kind = reference, "workspace"
			page = desktopplugins.NewHost(desktopplugins.HostBinding{Context: ctx, Client: controller, Scope: scope,
				UsageSources: r.pluginUsageSources, UsageChanges: r.pluginUsageChanges(ctx), UsageSample: r.pluginUsageSample()})
			if err := bind(page); err != nil {
				t.Fatal(err)
			}
			settle()
			asked := len(sent())
			node := controller.Target().String()

			sources, err := ask("usageSources")
			if list, _ := sources.([]any); err != nil || len(list) != 1 || at(list[0], "node") != node {
				t.Fatalf("the demo's usage sources are %v, %v", sources, err)
			}
			report, err := ask("readUsageChoices", month, quote(node))
			if err != nil {
				t.Fatal(err)
			}
			responses, _ := strconv.ParseUint(fmt.Sprint(at(report, "totals", "responses")), 10, 64)
			groups, _ := at(report, "groups").([]any)
			days, _ := at(report, "days").([]any)
			records, _ := at(report, "requests", "items").([]any)
			if responses == 0 || len(groups) != 3 || at(groups[0], "name") != preview.DemoProvider || len(days) != 30 || len(records) != 20 ||
				at(records[0], "provider_name") != preview.DemoProvider || at(report, "node") != node {
				t.Fatalf("the demo's usage holds %d responses in %d groups, %d days and %d records on %v", responses, len(groups), len(days), len(records), at(report, "node"))
			}
			inventory, err := ask("readUsageInventory", quote(node))
			providers, _ := at(inventory, "providers").([]any)
			projects, _ := at(inventory, "projects").([]any)
			if err != nil || len(providers) != 1 || at(providers[0], "name") != preview.DemoProvider || len(projects) != 3 || at(projects[0], "name") != "harbor-api" || at(inventory, "node") != node {
				t.Fatalf("the demo's usage chooses between %v, %v", inventory, err)
			}
			if _, err := ask("watchUsage", month, "false"); err != nil {
				t.Fatal(err)
			}
			event, err := ask("nextUsageChange", `""`)
			if observed := fmt.Sprint(at(event, "view", "report", "totals", "responses")); err != nil || at(event, "cursor") != "0" || observed == "0" || observed == "<nil>" || at(event, "view", "connected") != true {
				t.Fatalf("the Usage page's watch observes %v, %v", event, err)
			}
			// The page waits here while the switch is flipped back.
			go func() {
				value, err := ask("nextUsageChange", `"0"`)
				waiting <- usageAnswer{value, err}
			}()
			settle()
			if acted := sent()[asked:]; len(acted) != 0 {
				t.Errorf("the Usage page asked the Node in the demo: %q", acted)
			}
		})
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

		// The flip disposes the Usage page, which mounts again in its
		// initial state over the Node's own ledger: a filter chosen over the
		// demo's projects and models matches nothing there. The old mount's
		// host closes, so the call that waited on it ends; a mount that
		// reads no usage stays as it is.
		t.Run("usage", func(t *testing.T) {
			if changes != 2 || r.pluginUsageSample() != nil || page == nil {
				t.Fatalf("the switch told the plugin pages %d times; a sample %v", changes, r.pluginUsageSample() != nil)
			}
			asked := len(sent())
			read := func() bool {
				return slices.ContainsFunc(sent()[asked:], func(kind string) bool { return strings.HasSuffix(kind, "read_usage") })
			}
			reference := protocol.PluginReference{Name: "statistics"}
			info := protocol.DefaultPluginInfo()
			for _, summary := range controller.Snapshot().Plugins {
				if summary.Name == reference.Name {
					reference, info.Summary = packageReference(summary), summary
				}
			}
			extension := protocol.DefaultPluginExtension()
			extension.Actions = []protocol.PluginAction{protocol.PluginActionReadUsage, protocol.PluginActionReadConversation}
			info.Extension = &extension
			// The page is mounted for as long as the switch tells it; no
			// frame is drawn meanwhile. Beside it are the package's view
			// over a composer, which cannot read usage, and the page of a
			// package that declares no usage.
			fixture := func(name, surface string, info protocol.PluginInfo, host *desktopplugins.Host) *pluginMountedView {
				scope := protocol.DefaultPluginContext()
				scope.Package, scope.Surface.Kind = reference, surface
				mounted := &pluginMountedView{key: pluginMountKey{role: "navigation", name: name}, owner: r, info: info, scope: scope, host: host}
				mounted.ctx, mounted.cancel = context.WithCancel(ctx)
				r.pluginViews.mounts[mounted.key] = mounted
				return mounted
			}
			host := func(surface string) *desktopplugins.Host {
				scope := protocol.DefaultPluginContext()
				scope.Package, scope.Surface.Kind = reference, surface
				return desktopplugins.NewHost(desktopplugins.HostBinding{Context: ctx, Client: controller, Scope: scope,
					UsageSources: r.pluginUsageSources, UsageChanges: r.pluginUsageChanges(ctx), UsageSample: r.pluginUsageSample()})
			}
			usage := fixture("usage-fixture", "workspace", info, page)
			usage.bundle = &protocol.PluginDesktopBundle{Package: reference}
			figures := fixture("figures-fixture", "composer", info, host("composer"))
			plain := fixture("plain-fixture", "workspace", protocol.DefaultPluginInfo(), host("workspace"))
			defer func() {
				for _, mounted := range []*pluginMountedView{usage, figures, plain} {
					if r.pluginViews.mounts[mounted.key] == mounted {
						r.dropPluginView(mounted, true)
					}
					delete(r.pluginViews.mounts, mounted.key)
					delete(r.pluginViews.remounts, mounted.key)
				}
			}()
			if !pluginUsesUsage(usage) || pluginUsesUsage(figures) || pluginUsesUsage(plain) {
				t.Fatalf("usage is read by the page %v, the composer view %v, a page without the action %v",
					pluginUsesUsage(usage), pluginUsesUsage(figures), pluginUsesUsage(plain))
			}
			r.demoChanged()
			page = nil // closed with its mount

			// The page's mount is gone and closed, its bundle kept for the
			// next one; the wait on its host ended.
			if r.pluginViews.mounts[usage.key] != nil || usage.host != nil || usage.ctx.Err() == nil {
				t.Fatalf("the Usage page's mount outlived the switch: listed %v, host %v, context %v",
					r.pluginViews.mounts[usage.key] != nil, usage.host != nil, usage.ctx.Err())
			}
			if kept, ok := r.pluginViews.remounts[usage.key]; !ok || kept.Package != reference {
				t.Errorf("the next mount has no bundle to take: %v", ok)
			}
			select {
			case got := <-waiting:
				if got.err == nil {
					t.Errorf("the old page's wait went on after the switch: %v", got.value)
				}
			case <-ctx.Done():
				t.Fatal("the switch did not end the old page's wait")
			}
			// The mounts that read no usage are the ones they were.
			for _, mounted := range []*pluginMountedView{figures, plain} {
				if r.pluginViews.mounts[mounted.key] != mounted || mounted.host == nil || mounted.ctx.Err() != nil {
					t.Errorf("the switch remounted %s, which reads no usage", mounted.key.name)
				}
			}
			if _, kept := r.pluginViews.remounts[plain.key]; kept || len(r.pluginViews.remounts) != 1 {
				t.Errorf("%d mounts wait to be mounted again", len(r.pluginViews.remounts))
			}
			// A second switch finds no page to dispose.
			r.demoChanged()
			if len(r.pluginViews.remounts) != 1 || r.pluginViews.mounts[figures.key] != figures {
				t.Error("a switch without the page changed the mounts")
			}

			// The page mounted again has a new host, as mountPluginView
			// builds it: without a sample and without a watch, so the page
			// starts from its first query, on the Node's own ledger, which
			// a new profile has nothing in.
			page = host("workspace")
			if err := bind(page); err != nil {
				t.Fatal(err)
			}
			if _, err := ask("nextUsageChange", `""`); err == nil {
				t.Error("the new page inherited a watch")
			}
			if _, err := ask("watchUsage", month, "false"); err != nil {
				t.Fatal(err)
			}
			value, err := ask("nextUsageChange", `""`)
			got := usageAnswer{value, err}
			// The Node's watch settles on a report, or on the Node's refusal.
			for got.err == nil && at(got.value, "view", "report") == nil && !(read() && at(got.value, "view", "error") != nil) {
				value, err := ask("nextUsageChange", quote(fmt.Sprint(at(got.value, "cursor"))))
				got = usageAnswer{value, err}
			}
			if got.err != nil {
				t.Fatalf("the new page's watch failed: %v", got.err)
			}
			if report := at(got.value, "view", "report"); report != nil {
				if groups, _ := at(report, "groups").([]any); at(report, "totals", "responses") != "0" || len(groups) != 0 {
					t.Errorf("the profile's own usage is %v", report)
				}
			}
			if !read() {
				t.Errorf("the Usage page did not ask the Node after the switch: %q", sent()[asked:])
			}
			if err := page.Close(); err != nil {
				t.Error(err)
			}
			page = nil
			settle()
		})
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
