package shell

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// A session pane chosen from one host's row shows that host's session from
// then on: clicked, dropped, focused away and back, shown beside another
// pane without the focus, and held while its host is away, though another
// host lists a session of the same ID first. A layout read back keeps IDs
// alone, so its panes find their sessions by ID as before.
func TestPanesKeepTheirRowsHost(t *testing.T) {
	alone := &viewmodel.Conversation{Node: "local", ID: "alone", Title: "Alone"}
	localShared := &viewmodel.Conversation{Node: "local", ID: "shared", Title: "Local shared"}
	remoteShared := &viewmodel.Conversation{Node: "remote", ID: "shared", Title: "Remote shared"}
	listed := func(list ...*viewmodel.Conversation) *viewmodel.Data {
		return &viewmodel.Data{Unassigned: list, Workspace: &viewmodel.Workspace{}}
	}
	a := NewLive(listed(alone, localShared, remoteShared))
	a.Sidebar.Selected = alone.ID
	shared := session("shared")
	var post func(*ui.Context)
	titles := map[panes.Target]string{}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		a.View(c)
		if post != nil {
			post(c)
			post = nil
		}
		clear(titles)
		for _, m := range a.panes.Members(a.panes.Active) {
			titles[m] = a.paneTitle(c, m)
		}
	}, 1200, 820, opaque)
	tt.SetLocale("en")
	settle := func() { tt.Frame(); tt.Advance(time.Second); tt.Frame() }
	focus := func(target panes.Target) {
		post = func(c *ui.Context) { a.panes.handle(target).Focus(c) }
		settle()
	}
	settle()
	selected := func(cv *viewmodel.Conversation) bool {
		return a.Sidebar.Selected == cv.ID && a.Sidebar.SelectedNode() == cv.Node &&
			a.SelectedConversation() == cv && a.Sidebar.Current() == cv
	}

	// Click: a row selects its host's session, and the pane shows it.
	a.Sidebar.SelectOn(remoteShared.Node, remoteShared.ID)
	settle()
	if a.panes.Active != shared || a.paneSession(shared) != remoteShared || !selected(remoteShared) {
		t.Fatalf("the remote row's click shows %+v", a.paneSession(shared))
	}
	// The other host's row of the same ID is another choice.
	a.Sidebar.SelectOn(localShared.Node, localShared.ID)
	settle()
	if a.paneSession(shared) != localShared || !selected(localShared) {
		t.Fatalf("the local row's click shows %+v", a.paneSession(shared))
	}

	// Drop: the remote row dropped beside another pane, chosen from its own
	// local row, splits in the remote session.
	a.Sidebar.SelectOn(alone.Node, alone.ID)
	settle()
	post = func(c *ui.Context) { a.dropPane(c, session(alone.ID), shared, remoteShared.Node, panes.Right) }
	settle()
	if a.panes.Group(shared) < 0 || a.panes.Active != shared || a.paneSession(shared) != remoteShared || !selected(remoteShared) {
		t.Fatalf("the drop shows %+v, selected %q on %q", a.paneSession(shared), a.Sidebar.Selected, a.Sidebar.SelectedNode())
	}

	// Focus away: both panes show, the remote one without the focus.
	focus(session(alone.ID))
	if !selected(alone) || a.paneSession(shared) != remoteShared {
		t.Fatalf("after focusing %q: active=%+v selected=%q host=%q current=%p want=%p unfocused=%p want=%p", alone.ID, a.panes.Active, a.Sidebar.Selected, a.Sidebar.SelectedNode(), a.Sidebar.Current(), alone, a.paneSession(shared), remoteShared)
	}
	if titles[shared] != remoteShared.Title || titles[session(alone.ID)] != alone.Title {
		t.Fatalf("the split shows %v", titles)
	}

	// And back: by the pane itself, and by its ID alone as a tab or a
	// notification opens it.
	focus(shared)
	if !selected(remoteShared) {
		t.Fatalf("focus back selected %q on %q", a.Sidebar.Selected, a.Sidebar.SelectedNode())
	}
	focus(session(alone.ID))
	a.OpenConversation(shared.ID)
	settle()
	if a.panes.Active != shared || !selected(remoteShared) || a.paneSession(shared) != remoteShared {
		t.Fatalf("opening the ID selected %q on %q", a.Sidebar.Selected, a.Sidebar.SelectedNode())
	}

	// A pane's own drag carries no host and keeps the pane's.
	post = func(c *ui.Context) { a.dropPane(c, session(alone.ID), shared, "", panes.Left) }
	settle()
	if a.panes.Group(shared) < 0 || a.paneSession(shared) != remoteShared || !selected(remoteShared) {
		t.Fatalf("the pane's move shows %+v", a.paneSession(shared))
	}

	// Away: while the remote host does not list it, the pane is held for it
	// rather than given the local session of its ID, and it comes back.
	focus(session(alone.ID))
	a.ApplyData(listed(alone, localShared))
	settle()
	if a.panes.Group(shared) >= 0 || a.paneSession(shared) != nil {
		t.Fatalf("the away host's pane shows %+v", a.paneSession(shared))
	}
	a.ApplyData(listed(alone, localShared, remoteShared))
	settle()
	if a.panes.Group(shared) < 0 || a.paneSession(shared) != remoteShared || titles[shared] != remoteShared.Title {
		t.Fatalf("the returning host's pane shows %+v", a.paneSession(shared))
	}

	// A layout read back: its targets are IDs alone, and a new window finds
	// each pane's session by ID, the first of a shared one, as before.
	saved, err := json.Marshal(a.panes.Saved())
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(saved), remoteShared.Node) {
		t.Fatalf("the saved layout names a host: %s", saved)
	}
	var groups []*panes.Node
	if err := json.Unmarshal(saved, &groups); err != nil {
		t.Fatal(err)
	}
	restored := NewLive(listed(alone, localShared, remoteShared))
	restored.Sidebar.Selected = alone.ID
	restored.SetPrefs(Prefs{Panes: groups})
	if restored.panes.Group(shared) < 0 || restored.paneSession(shared) != localShared {
		t.Fatalf("the restored pane shows %+v", restored.paneSession(shared))
	}
	restored.focusPane(shared)
	if restored.Sidebar.SelectedNode() != "" || restored.SelectedConversation() != localShared {
		t.Fatal("a restored pane chose a host")
	}
}
