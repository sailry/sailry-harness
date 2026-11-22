package sidebar

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestConfirmDeleteKeepsShownHostAndRevision(t *testing.T) {
	row := &viewmodel.Conversation{ID: "shared", Node: "first", Revision: 5, Title: "First host session"}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{row}}, row.ID)
	var removed []*viewmodel.Conversation
	s.Actions = &Actions{Delete: func(targets []*viewmodel.Conversation) { removed = targets }}
	window := ui.NewTester(func(c *ui.Context) { s.confirmDelete(c, row) }, 300, 200)
	window.SetLocale("en")
	window.Frame()
	if !s.confirm.open || s.confirm.message != i18n.In("en", "session_delete_private_named", "name", row.Title) || len(removed) != 0 {
		t.Fatalf("single private session prompt = %q, removed = %v", s.confirm.message, removed)
	}
	row.Node, row.Revision = "second", 7
	s.confirm.run()
	if len(removed) != 1 || removed[0].ID != "shared" || removed[0].Node != "first" || removed[0].Revision != 5 {
		t.Fatalf("confirmation changed its target: %+v", removed)
	}
}

func TestSidebarBatchDeleteConfirmsCapturedTargetsOnce(t *testing.T) {
	first := &viewmodel.Conversation{Node: "local", ID: "shared", Revision: 5}
	second := &viewmodel.Conversation{Node: "remote", ID: "shared", Revision: 8}
	other := &viewmodel.Conversation{Node: "local", ID: "other", Revision: 12}
	targets := []*viewmodel.Conversation{first, second}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{first, second, other}}, first.ID)
	var removals [][]*viewmodel.Conversation
	s.Actions = &Actions{Delete: func(rows []*viewmodel.Conversation) { removals = append(removals, rows) }}
	var prompts []kit.Confirmation
	var answer func(int)
	const label = "Selected sessions"
	tt := ui.NewTester(func(c *ui.Context) {
		kit.BindConfirmations(c, func(request kit.Confirmation, done func(int)) {
			prompts = append(prompts, request)
			answer = done
		})
		ui.ButtonBase(c.Key("selection")).Label(label).Size(160, 28).ContextMenu(func(m *ui.Menu) {
			s.BatchSessionMenu(c, m, targets)
		})
		Dialogs(c, s)
	}, 300, 200)
	tt.SetLocale("en")
	tt.Frame()
	chooseDelete := func() {
		t.Helper()
		if err := tt.RightClick(label); err != nil {
			t.Fatal(err)
		}
		want := []string{i18n.In("en", "workspace_archive"), i18n.In("en", "session_delete")}
		if menu := tt.Menu(); !slices.Equal(menu, want) {
			t.Fatalf("batch menu = %v, want %v", menu, want)
		}
		if err := tt.ChooseMenuItem(i18n.In("en", "session_delete")); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
	}
	chooseDelete()
	wantMessage := i18n.In("en", "session_delete_many_confirm", "count", 2)
	if len(prompts) != 1 || prompts[0].Message != wantMessage || len(removals) != 0 {
		t.Fatalf("choosing delete: prompts=%v removals=%v, want one prompt %q before deleting", prompts, removals, wantMessage)
	}
	answer(0)
	tt.Frame()
	if s.confirm.open || len(prompts) != 1 || len(removals) != 0 {
		t.Fatal("cancel did not discard the batch")
	}
	chooseDelete()
	if len(prompts) != 2 || len(removals) != 0 {
		t.Fatal("opening another delete prompt submitted a deletion")
	}
	first.Node, first.Revision = "elsewhere", 6
	targets[1] = other
	s.SelectOn(other.Node, other.ID)
	answer(1)
	tt.Frame()
	tt.Frame()
	if len(prompts) != 2 || len(removals) != 1 || s.confirm.open || s.Selected != other.ID {
		t.Fatalf("confirmation: prompts=%d removals=%d open=%t selected=%q", len(prompts), len(removals), s.confirm.open, s.Selected)
	}
	got := removals[0]
	if len(got) != 2 || got[0].Node != "local" || got[0].ID != "shared" || got[0].Revision != 5 ||
		got[1].Node != "remote" || got[1].ID != "shared" || got[1].Revision != 8 {
		t.Fatalf("batch deletion did not keep both host revisions: %+v", got)
	}
}

func TestPreviewBatchDeletePreservesRemainingHost(t *testing.T) {
	selected := &viewmodel.Conversation{Node: "local", ID: "shared"}
	remaining := &viewmodel.Conversation{Node: "remote", ID: "shared"}
	projectRow := &viewmodel.Conversation{Node: "local", ID: "project-session"}
	project := &viewmodel.Project{ID: "project", Conversations: []*viewmodel.Conversation{projectRow}}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{selected, remaining}, Projects: []*viewmodel.Project{project}}, "")
	s.SelectOn(selected.Node, selected.ID)
	tt := ui.NewTester(func(c *ui.Context) { s.confirmDelete(c, selected, projectRow) }, 300, 200)
	tt.Frame()
	if s.Current() != selected || len(project.Conversations) != 1 || len(s.Data.Unassigned) != 2 {
		t.Fatal("prompt changed the preview before confirmation")
	}
	s.confirm.run()
	if s.Current() != remaining || len(project.Conversations) != 0 || !slices.Equal(s.Data.Unassigned, []*viewmodel.Conversation{remaining}) {
		t.Fatal("confirming the batch did not preserve the remaining host's session")
	}
}
