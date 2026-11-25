package desktop

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestBrowserTabOpenUsesEffectiveShortcut(t *testing.T) {
	app := shell.NewWithAssembly(preview.Load(), shell.Assemble(Register))
	state := app.Panel
	state.Live = true
	state.BrowserActions = &panel.BrowserActions{
		Page: func(string) panel.BrowserPage { return panel.BrowserPage{} },
		View: func(*ui.Context, string) bool { return true },
	}
	app.PanelOpen = true
	state.Open(panel.Browser)
	tester := ui.NewTester(app.View, 1280, 800)
	tester.SetLocale("en")
	address := i18n.In("en", "browser_address")
	focusAddress := func() {
		t.Helper()
		tester.Advance(time.Second)
		tester.Frame()
		if err := tester.Click(address); err != nil || !tester.Focused(address) {
			t.Fatalf("could not focus the Browser address: %v, %q", err, tester.Texts())
		}
	}
	press := func(mods ui.Modifiers, key ui.Key, want int) {
		t.Helper()
		focusAddress()
		tester.Key(mods, key)
		tester.Frame()
		if len(state.Tabs) != want || state.Active != state.Tabs[want-1] {
			t.Fatalf("pressing %v/%v left tabs %v (active %v); want %d", mods, key, state.Tabs, state.Active, want)
		}
	}
	press(ui.Cmd, ui.KeyT, 2)
	keys := &app.Settings.Keys
	id := settings.PanelShortcutID(string(panel.Browser))
	keys.Record(id)
	mods := ui.Ctrl | ui.Alt
	if err := keys.Capture(settings.Chord{Mods: mods, Key: ui.KeyJ}); err != "" {
		t.Fatalf("rebinding Browser shortcut: %s", err)
	}
	press(ui.Cmd, ui.KeyT, 2)
	press(mods, ui.KeyJ, 3)
	keys.Record(id)
	keys.Clear()
	press(mods, ui.KeyJ, 3)
}

func TestLiveBrowserAddressAcrossNavigationAndTabs(t *testing.T) {
	const host = "http://127.0.0.1:8080"
	s := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, shell.Assemble(Register)).Panel
	s.New(panel.Browser)
	first := s.Active
	second := s.AddBrowser(false)
	pages := map[string]panel.BrowserPage{
		first.ID:  {Address: host + "/first", Title: "First page"},
		second.ID: {Address: host + "/second", Title: "Second page"},
	}
	attempts := 0
	s.BrowserActions = &panel.BrowserActions{
		Page:   func(id string) panel.BrowserPage { return pages[id] },
		View:   func(*ui.Context, string) bool { return true },
		Reload: func(string) { t.Fatal("the address field reloaded the page") },
		Navigate: func(id, address string) bool {
			attempts++
			if id != first.ID {
				t.Fatalf("submitted on tab %q, want %q", id, first.ID)
			}
			if address == "custom://rejected" {
				return false
			}
			if address != host+"/submit" {
				t.Fatalf("navigation to %q, want %q", address, host+"/submit")
			}
			pages[id] = panel.BrowserPage{Address: address, Loading: true}
			return true
		},
	}
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { panel.View(c, s, nil, nil) })
	}, 700, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	field := func(want string) ui.AccessInfo {
		t.Helper()
		tester.Frame()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleTextField && info.Name == i18n.In("en", "browser_address") {
				if info.Value != want {
					t.Fatalf("address field = %q, want %q", info.Value, want)
				}
				return info
			}
		}
		t.Fatal("browser address field missing")
		return ui.AccessInfo{}
	}
	typeAddress := func(text string) {
		tester.Key(ui.Cmd, ui.KeyA)
		tester.Type(text)
	}
	field(host + "/first")
	if active, _ := tester.TextInputActive(); !active {
		t.Fatal("the new browser's address field did not take focus")
	}
	typeAddress("127.0.0.1:8080/submit")
	tester.Key(0, ui.KeyEnter)
	field(host + "/submit")
	pages[first.ID] = panel.BrowserPage{Address: host + "/redirected", Title: "Redirected"}
	field(host + "/redirected")
	if active, _ := tester.TextInputActive(); !active {
		t.Fatal("the redirect took focus from the address field")
	}

	const draft = "custom://rejected"
	tester.Key(ui.Cmd, ui.KeyA)
	pages[first.ID] = panel.BrowserPage{Address: host + "/redirected#typing", Title: "Typing route"}
	tester.Type(draft)
	tester.Key(0, ui.KeyEnter)
	field(draft)
	pages[first.ID] = panel.BrowserPage{Address: host + "/redirected#active", Title: "Active route"}
	field(draft)
	s.Show(second)
	field(host + "/second")
	pages[first.ID] = panel.BrowserPage{Address: host + "/redirected#background", Title: "Background route"}
	field(host + "/second")
	if title := s.Title(nil, first); title != "Background route" {
		t.Fatalf("the background tab is titled %q", title)
	}
	s.Show(first)
	info := field(draft)
	tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
	tester.Key(0, ui.KeyEscape)
	field(host + "/redirected#background")
	if attempts != 2 {
		t.Fatalf("the address field submitted %d navigations, want 2", attempts)
	}
}
