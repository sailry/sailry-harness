package shell

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type overviewLifecycleFixture struct {
	app      *App
	tester   *ui.Tester
	opened   []bool
	loading  bool
	complete bool
}

func newOverviewLifecycleFixture(t *testing.T, width int) *overviewLifecycleFixture {
	t.Helper()
	cv := &viewmodel.Conversation{
		ID: "overview-session", Title: "Overview fixture",
		Turns: []conversation.Turn{{ID: "turn", User: "Fixture input", Reply: conversation.Reply{Status: conversation.Completed}}},
	}
	f := &overviewLifecycleFixture{}
	f.app = NewLive(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv}, Workspace: &viewmodel.Workspace{}})
	f.app.Sidebar.Selected = cv.ID
	f.app.NavOpen = false
	f.app.Hooks.Overview = func(*ui.Context, *viewmodel.Conversation) *composer.Overview {
		return &composer.Overview{
			Session: true, Loading: f.loading,
			Opened: func(open bool) {
				f.opened = append(f.opened, open)
				if open && !f.complete {
					f.loading = true
				}
			},
		}
	}
	f.tester = ui.NewTesterWith(f.app.View, width, 820, opaque)
	f.tester.SetLocale("en")
	f.settle()
	return f
}

func (f *overviewLifecycleFixture) settle() {
	for range 4 {
		f.tester.Frame()
		f.tester.Advance(200 * time.Millisecond)
	}
	f.tester.Frame()
}

func TestOverviewLifecycle(t *testing.T) {
	t.Run("first click stays open while an empty inventory loads", func(t *testing.T) {
		f := newOverviewLifecycleFixture(t, 1100)
		entry := f.app.overviews["overview-session"]
		if entry == nil || entry.state.Docked || len(f.opened) != 0 {
			t.Fatalf("initial overview state = %+v, visibility notifications %v", entry, f.opened)
		}
		button, ok := headerButton(f.tester, i18n.In("en", "chat_assets_title"))
		if !ok {
			t.Fatal("the narrow conversation has no overview button")
		}
		f.tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		f.settle()
		if f.app.overviews["overview-session"] != entry || !entry.state.Open() || entry.state.Docked || len(f.opened) != 1 || !f.opened[0] {
			t.Fatalf("first click lost its popover: entry %+v, visibility notifications %v", entry, f.opened)
		}
		f.loading, f.complete = false, true
		f.settle()
		if !entry.state.Open() || entry.state.Docked || len(f.opened) != 1 {
			t.Fatalf("empty inventory changed the presentation: entry %+v, visibility notifications %v", entry, f.opened)
		}
		if !f.tester.HasText(i18n.In("en", "chat_assets_waiting")) {
			t.Fatal("the open popover does not show its empty inventory")
		}
	})

	t.Run("empty inventory keeps its card and layout while loading", func(t *testing.T) {
		f := newOverviewLifecycleFixture(t, 1800)
		entry := f.app.overviews["overview-session"]
		if entry == nil || !entry.state.Docked || len(f.opened) != 1 || !f.opened[0] {
			t.Fatalf("initial card state = %+v, visibility notifications %v", entry, f.opened)
		}
		before, ok := f.tester.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
		if !ok {
			t.Fatal("the conversation has no composer")
		}
		f.loading, f.complete = false, true
		f.settle()
		after, ok := f.tester.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
		if !ok || before.Bounds.X != after.Bounds.X || before.Bounds.W != after.Bounds.W || !entry.state.Docked || len(f.opened) != 1 {
			t.Fatalf("empty inventory moved the composer from %v to %v; visibility notifications %v", before.Bounds, after.Bounds, f.opened)
		}
		if _, ok := f.tester.AccessByRole(ui.RoleComplementary, i18n.In("en", "chat_assets_title")); !ok {
			t.Fatal("the empty inventory lost its docked card")
		}
	})

	t.Run("leaving a visible overview closes its owner", func(t *testing.T) {
		f := newOverviewLifecycleFixture(t, 1800)
		f.app.Page = SettingsPage
		f.settle()
		if _, exists := f.app.overviews["overview-session"]; exists {
			t.Fatal("the hidden conversation kept its overview presentation")
		}
		if len(f.opened) != 2 || !f.opened[0] || f.opened[1] {
			t.Fatalf("leaving the conversation did not close its overview owner: %v", f.opened)
		}
	})

	t.Run("sweeping a removed owner uses the callback that opened it", func(t *testing.T) {
		f := newOverviewLifecycleFixture(t, 1800)
		f.app.Hooks.Overview = func(*ui.Context, *viewmodel.Conversation) *composer.Overview {
			return &composer.Overview{}
		}
		f.settle()
		f.app.Page = SettingsPage
		f.settle()
		if len(f.opened) != 2 || !f.opened[0] || f.opened[1] {
			t.Fatalf("removing the source lost its visibility callback: %v", f.opened)
		}
	})

	t.Run("shared overview values keep their original callback", func(t *testing.T) {
		app := NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
		entry := app.overviewEntry("shared-session")
		var opened []bool
		value := &composer.Overview{Session: true, Opened: func(open bool) { opened = append(opened, open) }}
		first := entry.observe(value)
		second := entry.observe(value)
		first.Opened(true)
		second.Opened(true)
		second.Opened(false)
		entry.close()
		value.Opened(true)
		if len(opened) != 3 || !opened[0] || opened[1] || !opened[2] {
			t.Fatalf("observing a shared value wrapped or duplicated its callback: %v", opened)
		}
	})
}
