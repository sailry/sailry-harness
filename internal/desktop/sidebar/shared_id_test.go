package sidebar

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Two hosts may list a session or a project of one ID: every order keeps
// each host's own, rather than resolving both to the one listed last.
func TestOrdersKeepEachHostsSharedID(t *testing.T) {
	now := time.Now()
	local := &viewmodel.Conversation{Node: "local", ID: "shared", Title: "b", Updated: now, Pinned: true}
	remote := &viewmodel.Conversation{Node: "remote", ID: "shared", Title: "a", Updated: now.Add(-time.Minute), Pinned: true}
	other := &viewmodel.Conversation{Node: "local", ID: "other", Title: "c", Updated: now.Add(time.Minute)}
	localProject := &viewmodel.Project{Host: "local", ID: "project"}
	remoteProject := &viewmodel.Project{Host: "remote", ID: "project"}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{local, remote, other}, Projects: []*viewmodel.Project{localProject, remoteProject}}, "")
	for _, by := range []Sort{SortCreated, SortUpdated, SortName} {
		*s.Sort = int(by)
		got := s.sessions(s.Data.Unassigned)
		if len(got) != 3 || !slices.Contains(got, local) || !slices.Contains(got, remote) || !slices.Contains(got, other) {
			t.Errorf("sort %v: sessions %v", by, got)
		}
		if projects := s.projects(); len(projects) != 2 || projects[0] == projects[1] {
			t.Errorf("sort %v: projects %v", by, projects)
		}
	}
	if pinned := s.pinned(); len(pinned) != 2 || pinned[0] != local || pinned[1] != remote {
		t.Errorf("pinned %v", pinned)
	}
	if RowKey(local) == RowKey(remote) {
		t.Error("the hosts' rows share a key")
	}
}

// A row's drag, reorder, rename and project act on the session of the row's
// host, though the host listed first has a session of the same ID.
func TestRowsActOnTheirHostsSharedID(t *testing.T) {
	local := &viewmodel.Conversation{Node: "local", ID: "shared", Title: "local", Pinned: true}
	localOther := &viewmodel.Conversation{Node: "local", ID: "other"}
	remote := &viewmodel.Conversation{Node: "remote", ID: "shared", Title: "remote", Pinned: true}
	remoteOther := &viewmodel.Conversation{Node: "remote", ID: "other"}
	localProject := &viewmodel.Project{Host: "local", ID: "project", Name: "Local", Conversations: []*viewmodel.Conversation{local, localOther}}
	remoteProject := &viewmodel.Project{Host: "remote", ID: "project", Name: "Remote", Conversations: []*viewmodel.Conversation{remote, remoteOther}}
	s := New(&viewmodel.Data{Projects: []*viewmodel.Project{localProject, remoteProject}}, "")

	if s.Data.ProjectHolding(remote) != remoteProject || s.Data.ProjectHolding(localOther) != localProject {
		t.Error("a session resolved to the other host's project")
	}
	row := panes.Drag{Target: panes.Target{Kind: panes.Session, ID: "shared"}, Node: "remote"}
	if got := s.dragged(row); got != remote {
		t.Errorf("a remote row's drag carries %+v", got)
	}
	if got := s.dragged(panes.Drag{Target: row.Target}); got != local {
		t.Errorf("a pane's drag carries %+v, not the session its pane shows", got)
	}
	if !s.sessionDrag(remote, remoteOther) || s.sessionDrag(local, remoteOther) {
		t.Error("a project's reorder took the wrong host's session")
	}

	s.MoveSession(remote, remoteOther, true)
	if !slices.Equal(remoteProject.Conversations, []*viewmodel.Conversation{remoteOther, remote}) ||
		!slices.Equal(localProject.Conversations, []*viewmodel.Conversation{local, localOther}) {
		t.Errorf("move: local %v, remote %v", localProject.Conversations, remoteProject.Conversations)
	}

	if pinned := s.pinned(); len(pinned) != 2 || pinned[0] != local {
		t.Fatalf("pinned %v", pinned)
	}
	s.MovePin(local, remote, true)
	if pinned := s.pinned(); len(pinned) != 2 || pinned[0] != remote || pinned[1] != local {
		t.Errorf("pins of one ID did not reorder: %v", pinned)
	}

	s.StartRename(remote)
	if s.renamingRow(local) || !s.renamingRow(remote) {
		t.Error("the rename's input went to the other host's row")
	}
	if copied := *remote; !s.renamingRow(&copied) {
		t.Error("a new projection of the renamed session lost its input")
	}
}

// A click on one host's row selects that host's session and marks its row
// alone, though the other host lists a session of the same ID first.
func TestSelectionKeepsTheClickedHost(t *testing.T) {
	local := &viewmodel.Conversation{Node: "local", ID: "shared", Title: "Local shared"}
	remote := &viewmodel.Conversation{Node: "remote", ID: "shared", Title: "Remote shared"}
	other := &viewmodel.Conversation{Node: "local", ID: "other", Title: "Other"}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{local, remote, other}}, other.ID)
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { View(c, s) })
	}, 300, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tt.SetLocale("en")
	settle := func() { tt.Frame(); tt.Advance(time.Second); tt.Frame() }
	settle()

	if err := tt.Click(remote.Title); err != nil {
		t.Fatal(err)
	}
	settle()
	if s.Selected != "shared" || s.SelectedNode() != "remote" || s.Current() != remote || s.current != remote {
		t.Fatalf("remote row selected %q on %q, current %+v", s.Selected, s.SelectedNode(), s.Current())
	}
	if !s.TakeShown() {
		t.Error("the click did not ask the shell to show the session")
	}

	if err := tt.Click(local.Title); err != nil {
		t.Fatal(err)
	}
	settle()
	if s.Selected != "shared" || s.SelectedNode() != "local" || s.current != local {
		t.Fatalf("local row selected %q on %q", s.Selected, s.SelectedNode())
	}

	// Setting Selected to another ID, as recovery and the shell do, drops the
	// host; an ID alone finds the first of it, as before.
	s.SelectOn("remote", "shared")
	s.Selected = other.ID
	if s.SelectedNode() != "" || s.Current() != other {
		t.Error("a bare ID kept the previous selection's host")
	}
	s.Selected = "shared"
	if s.Current() != local {
		t.Error("a bare shared ID did not find its first session")
	}
	// A named host's session missing from the rows marks none of another host.
	s.SelectOn("gone", "shared")
	if s.Current() != nil {
		t.Error("an unlisted host's selection marked another host's row")
	}
}
