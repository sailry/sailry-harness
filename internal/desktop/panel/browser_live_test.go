package panel

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestLiveBrowserAddressAcrossNavigationAndTabs(t *testing.T) {
	const host = "http://127.0.0.1:8080"
	s := New(nil)
	s.Live = true
	s.New(Browser)
	first := s.Active
	second := s.AddBrowser(false)
	pages := map[string]BrowserPage{
		first.ID:  {Address: host + "/first", Title: "First page"},
		second.ID: {Address: host + "/second", Title: "Second page"},
	}
	attempts := 0
	s.BrowserActions = &BrowserActions{
		Page:   func(id string) BrowserPage { return pages[id] },
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
			pages[id] = BrowserPage{Address: address, Loading: true}
			return true
		},
	}
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { View(c, s, nil, nil) })
	}, 700, 400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	field := func(want string) ui.AccessInfo {
		t.Helper()
		tester.Frame()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleTextField && info.Name == en("browser_address") {
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
	pages[first.ID] = BrowserPage{Address: host + "/redirected", Title: "Redirected"}
	field(host + "/redirected")
	if active, _ := tester.TextInputActive(); !active {
		t.Fatal("the redirect took focus from the address field")
	}

	const draft = "custom://rejected"
	tester.Key(ui.Cmd, ui.KeyA)
	pages[first.ID] = BrowserPage{Address: host + "/redirected#typing", Title: "Typing route"}
	tester.Type(draft)
	tester.Key(0, ui.KeyEnter)
	field(draft)
	pages[first.ID] = BrowserPage{Address: host + "/redirected#active", Title: "Active route"}
	field(draft)
	s.Show(second)
	field(host + "/second")
	pages[first.ID] = BrowserPage{Address: host + "/redirected#background", Title: "Background route"}
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
