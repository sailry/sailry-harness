package sidebar

import (
	"fmt"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestSidebarSelectionGestures(t *testing.T) {
	for _, archived := range []bool{false, true} {
		t.Run(fmt.Sprintf("archived=%t", archived), func(t *testing.T) {
			local := &viewmodel.Conversation{ID: "shared", Node: "local", Title: "Local session", Archived: archived}
			remote := &viewmodel.Conversation{ID: "shared", Node: "remote", Title: "Remote session", Archived: archived}
			other := &viewmodel.Conversation{ID: "other", Node: "local", Title: "Other session", Archived: archived}
			s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{local, remote, other}}, "")
			s.Archived = archived
			*s.Sort = int(SortCreated)
			tt := selectionTester(s)

			clickSelectionRow(t, tt, local.Title, 0)
			if s.Current() != local || !s.TakeShown() {
				t.Fatal("plain click did not open the local session")
			}
			clickSelectionRow(t, tt, remote.Title, ui.Cmd)
			assertSelectedRows(t, tt, local.Title, remote.Title)
			clickSelectionRow(t, tt, remote.Title, ui.Ctrl)
			assertSelectedRows(t, tt, local.Title)
			clickSelectionRow(t, tt, remote.Title, ui.Ctrl)
			assertSelectedRows(t, tt, local.Title, remote.Title)
			if !slices.Equal(s.menuTargets(remote), []*viewmodel.Conversation{local, remote}) {
				t.Fatal("selection did not retain both hosts' sessions with the shared ID")
			}

			batchAction, singleAction := "workspace_archive", "workspace_rename"
			if archived {
				batchAction, singleAction = "backup_unarchive", "content_copy"
			}
			if err := tt.RightClick(remote.Title); err != nil {
				t.Fatal(err)
			}
			if items := tt.Menu(); !slices.Contains(items, i18n.In("en", batchAction)) || slices.Contains(items, i18n.In("en", singleAction)) {
				t.Fatalf("right click on the selection opened the wrong menu: %v", items)
			}
			assertSelectedRows(t, tt, local.Title, remote.Title)
			tt.CloseMenu()
			if err := tt.RightClick(other.Title); err != nil {
				t.Fatal(err)
			}
			if items := tt.Menu(); !slices.Contains(items, i18n.In("en", singleAction)) {
				t.Fatalf("right click on an unselected row did not show its single-session menu: %v", items)
			}
			assertSelectedRows(t, tt, other.Title)
			tt.CloseMenu()
			clickSelectionRow(t, tt, local.Title, ui.Shift)
			assertSelectedRows(t, tt, local.Title, remote.Title, other.Title)
			if s.Current() != local || s.TakeShown() {
				t.Fatal("a selection gesture opened another session")
			}
			clickSelectionRow(t, tt, remote.Title, 0)
			assertSelectedRows(t, tt, remote.Title)
			if s.Current() != remote || !s.TakeShown() {
				t.Fatal("plain click did not open the remote session")
			}
		})
	}
}

func TestSidebarSelectionFollowsVisibleOrder(t *testing.T) {
	project := &viewmodel.Project{ID: "visible", Name: "Zen project"}
	for _, title := range []string{"Echo", "Delta", "Charlie", "Alpha", "Foxtrot", "Bravo"} {
		project.Conversations = append(project.Conversations, &viewmodel.Conversation{ID: title, Title: title})
	}
	chat := &viewmodel.Conversation{ID: "chat", Title: "Top session"}
	end := &viewmodel.Conversation{ID: "bottom", Title: "Bottom session"}
	old := &viewmodel.Conversation{ID: "archived", Title: "Archived session", Archived: true}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{chat}, Projects: []*viewmodel.Project{
		project,
		{ID: "hidden", Name: "Hidden project", Conversations: []*viewmodel.Conversation{{ID: "hidden", Title: "Hidden session"}}},
		{ID: "end", Name: "ZZ Last project", Conversations: []*viewmodel.Conversation{end, old}},
	}}, "")
	*s.Sort = int(SortName)
	s.SetCollapsed([]string{"hidden"})
	tt := selectionTester(s)
	clickSelectionRow(t, tt, chat.Title, 0)
	clickSelectionRow(t, tt, end.Title, ui.Shift)
	assertSelectedRows(t, tt, chat.Title, "Alpha", "Bravo", "Charlie", "Delta", "Echo", end.Title)
	if targets := s.menuTargets(end); len(targets) != Initial+2 {
		t.Fatalf("context menu has %d targets from the collapsed and limited order", len(targets))
	}
	clickSelectionRow(t, tt, i18n.In("en", "sidebar_sessions_more"), 0)
	clickSelectionRow(t, tt, end.Title, ui.Shift)
	assertSelectedRows(t, tt, chat.Title, "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", end.Title)

	s.Archived = true
	tt.Frame()
	if targets := s.menuTargets(old); !slices.Equal(targets, []*viewmodel.Conversation{old}) {
		t.Fatalf("archived view retained active targets: %v", targets)
	}
	s.Archived = false
	tt.Frame()
	assertSelectedRows(t, tt, chat.Title)
}

func TestSidebarSelectionKeepsPinnedPositions(t *testing.T) {
	pin := &viewmodel.Conversation{ID: "pin", Node: "local", Title: "Pinned session", Pinned: true}
	chat := &viewmodel.Conversation{ID: "chat", Title: "Unassigned session"}
	before := &viewmodel.Conversation{ID: "before", Title: "Before pinned"}
	after := &viewmodel.Conversation{ID: "after", Title: "After pinned"}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{chat}, Projects: []*viewmodel.Project{
		{ID: "project", Name: "Project", Conversations: []*viewmodel.Conversation{before, pin, after}},
	}}, "")
	*s.Sort = int(SortCreated)
	tt := selectionTester(s)
	var copies []ui.Rect
	for _, row := range tt.AccessAll() {
		if row.Role == ui.RoleButton && row.Name == pin.Title {
			copies = append(copies, row.Bounds)
		}
	}
	if len(copies) != 2 {
		t.Fatalf("got %d pinned rows, want 2", len(copies))
	}
	pinned, original := copies[0], copies[1]
	tt.RightClickAt(original.X+40, original.Y+original.H/2)
	assertSelectedRows(t, tt, pin.Title, pin.Title)
	tt.CloseMenu()
	clickSelectionRow(t, tt, after.Title, ui.Shift)
	assertSelectedRows(t, tt, pin.Title, pin.Title, after.Title)
	if targets := s.menuTargets(pin); !slices.Equal(targets, []*viewmodel.Conversation{pin, after}) {
		t.Fatalf("right click on a pinned copy got wrong targets: %v", targets)
	}

	// The copies can be distinct endpoints without duplicating menu targets.
	tt.ClickAt(pinned.X+40, pinned.Y+pinned.H/2)
	tt.ClickAtWith(ui.Shift, original.X+40, original.Y+original.H/2)
	assertSelectedRows(t, tt, pin.Title, chat.Title, before.Title, pin.Title)
	if targets := s.menuTargets(pin); !slices.Equal(targets, []*viewmodel.Conversation{pin, chat, before}) {
		t.Fatalf("range between pinned copies got wrong targets: %v", targets)
	}
}

func selectionTester(s *State) *ui.Tester {
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { View(c, s) })
	}, 360, 800, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	return tt
}

func clickSelectionRow(t *testing.T, tt *ui.Tester, label string, mods ui.Modifiers) {
	t.Helper()
	tt.Advance(time.Second)
	if err := tt.ClickWith(mods, label); err != nil {
		t.Fatal(err)
	}
}

func assertSelectedRows(t *testing.T, tt *ui.Tester, want ...string) {
	t.Helper()
	var got []string
	for _, row := range tt.AccessAll() {
		if row.Role == ui.RoleButton && row.Checked {
			got = append(got, row.Name)
		}
	}
	if !slices.Equal(got, want) {
		t.Fatalf("selected rows %q, want %q", got, want)
	}
}
