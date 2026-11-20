package sidebar

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestSidebarArchiveMode(t *testing.T) {
	chat := &viewmodel.Conversation{ID: "chat", Title: "Current chat"}
	oldChat := &viewmodel.Conversation{ID: "old-chat", Title: "Archived chat", Archived: true}
	first := &viewmodel.Conversation{ID: "first", Title: "Current first", Pinned: true}
	oldFirst := &viewmodel.Conversation{ID: "old-first", Title: "Archived first", Archived: true}
	second := &viewmodel.Conversation{ID: "second", Title: "Current second"}
	oldSecond := &viewmodel.Conversation{ID: "old-second", Title: "Archived second", Archived: true}
	data := &viewmodel.Data{
		Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{chat, oldChat},
		Projects: []*viewmodel.Project{
			{ID: "one", Host: "local", Name: "First project", Conversations: []*viewmodel.Conversation{first, oldFirst}},
			{ID: "two", Host: "remote", Name: "Second project", Conversations: []*viewmodel.Conversation{second, oldSecond}},
		},
	}
	s := New(data, first.ID)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { View(c, s) })
	}, 300, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.SetLocale("en")
	settle := func() { tt.Frame(); tt.Advance(time.Second); tt.Frame() }
	settle()
	for _, archived := range []bool{true, false, true} {
		if err := tt.Click(i18n.In("en", "sidebar_sort")); err != nil {
			t.Fatal(err)
		}
		settle()
		for _, order := range Sorts {
			if !tt.HasText(i18n.In("en", order.Label)) {
				t.Fatalf("archive choices replaced sort option %q", order.Label)
			}
		}
		key := "sidebar_sessions_active"
		if archived {
			key = "sidebar_sessions_archived"
		}
		if err := tt.Click(i18n.In("en", key)); err != nil {
			t.Fatal(err)
		}
		settle()
		if s.Archived != archived || s.Selected != first.ID || data.ProjectOf(first.ID) != data.Projects[0] {
			t.Fatal("filtering changed selection or project ownership")
		}
		order := s.Order()
		for _, cv := range []*viewmodel.Conversation{chat, oldChat, first, oldFirst, second, oldSecond} {
			want := cv.Archived == archived
			if slices.Contains(order, cv.ID) != want || tt.HasText(cv.Title) != want {
				t.Fatalf("archive mode %t has wrong session %q: %v", archived, cv.ID, order)
			}
		}
	}
	if err := tt.RightClick(oldSecond.Title); err != nil {
		t.Fatal(err)
	}
	if err := tt.ChooseMenuItem(i18n.In("en", "backup_unarchive")); err != nil {
		t.Fatal(err)
	}
	settle()
	if oldSecond.Archived || slices.Contains(s.Order(), oldSecond.ID) || data.ProjectOf(oldSecond.ID) != data.Projects[1] || s.Selected != first.ID {
		t.Fatal("restoring an archived row changed another session or its project")
	}
}

func TestSidebarArchiveGuardAndCapturedTarget(t *testing.T) {
	for _, scenario := range []struct {
		name   string
		status viewmodel.Status
		queue  []composer.Queued
		busy   bool
	}{
		{name: "running", status: viewmodel.Running},
		{name: "waiting", status: viewmodel.Waiting},
		{name: "held queue", queue: []composer.Queued{{}}},
		{name: "new Node activity", busy: true},
		{name: "idle"},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			cv := &viewmodel.Conversation{ID: "first", Title: "First session", Status: scenario.status, Queue: scenario.queue}
			other := &viewmodel.Conversation{ID: "other", Title: "Other session"}
			data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}, Projects: []*viewmodel.Project{
				{ID: "first-project", Name: "First project", Conversations: []*viewmodel.Conversation{cv}},
				{ID: "other-project", Name: "Other project", Conversations: []*viewmodel.Conversation{other}},
			}}
			s := New(data, other.ID)
			busy := scenario.busy
			var archived *viewmodel.Conversation
			s.Actions = &Actions{CanArchive: func(*viewmodel.Conversation) bool { return !busy }, Archive: func(target *viewmodel.Conversation) { archived = target }}
			ask := false
			tt := ui.NewTesterWith(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() { View(c, s) })
				if ask {
					ask = false
					s.archive(c, cv)
				}
			}, 300, 650, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
			tt.Frame()
			row, ok := tt.Access(cv.Title)
			if !ok {
				t.Fatal("session row is missing")
			}
			tt.Move(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
			tt.Frame()
			button, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "workspace_archive"))
			blocked := scenario.name != "idle"
			if !ok || button.Disabled != blocked {
				t.Fatalf("archive button availability = %+v, found %t", button, ok)
			}
			ask = true
			tt.Frame()
			if blocked {
				if s.confirm.open || archived != nil {
					t.Fatal("a busy session opened archive confirmation")
				}
				return
			}
			if !s.confirm.open {
				t.Fatal("idle session did not ask for confirmation")
			}
			busy = true
			s.confirm.run()
			if archived != nil {
				t.Fatal("work admitted during confirmation was archived")
			}
			busy = false
			s.confirm.run()
			if archived != cv || s.Selected != other.ID || data.ProjectOf(archived.ID) != data.Projects[0] {
				t.Fatal("confirmation used the selected session instead of its captured target")
			}
		})
	}
}

func TestPreviewArchiveRetainsSelection(t *testing.T) {
	cv := &viewmodel.Conversation{ID: "selected", Title: "Selected session"}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{cv, {ID: "other", Title: "Other project session"}}}, cv.ID)
	tt := ui.NewTester(func(c *ui.Context) { s.archive(c, cv) }, 300, 200)
	tt.Frame()
	s.confirm.run()
	if !cv.Archived || s.Selected != cv.ID {
		t.Fatal("archiving navigated away from the open transcript")
	}
}
