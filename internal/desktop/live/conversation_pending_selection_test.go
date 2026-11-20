package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Live acceptance and external callers open a session before the Node
// inventory lists it. The request is honoured when the session appears, but a
// sidebar navigation made in the meantime replaces it.
func TestPendingOpenSessionYieldsToLaterNavigation(t *testing.T) {
	for _, test := range []struct {
		name     string
		navigate bool
	}{{"unchanged", false}, {"manual", true}} {
		t.Run(test.name, func(t *testing.T) {
			full := selectionSnapshot("Earlier chat", "Manual chat", "Live run")
			full.Sessions[2].Activity.Run = &protocol.ConversationRun{
				Session: full.Sessions[2].ID, Turn: protocol.TurnID{9}, Sequence: 1, Revision: 1,
				Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning,
			}
			manual, opened := full.Sessions[1].ID.String(), full.Sessions[2].ID.String()
			initial := full
			initial.Sessions = full.Sessions[:2]
			f := selectionWindow(t, initial)
			r, app := f.r, f.r.App
			app.Sidebar.Selected = opened
			r.OpenSession(opened)
			f.window.Frame()
			if r.pendingSession != opened {
				t.Fatalf("the unlisted session's request was lost before it appeared: selected %q", app.Sidebar.Selected)
			}
			want := opened
			if test.navigate {
				row, ok := f.window.AccessByRole(ui.RoleButton, "Manual chat")
				if !ok || row.Offscreen || row.Bounds.W <= 0 || row.Bounds.H <= 0 {
					t.Fatalf("the sidebar row is not clickable: %q", f.window.Texts())
				}
				f.window.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
				f.window.Frame()
				if app.FocusedConversation() != manual {
					t.Fatalf("the sidebar click showed %q", app.FocusedConversation())
				}
				want = manual
			}
			full.Cursor = initial.Cursor + 1
			f.update(t, full.Cursor, protocol.Update{Kind: "snapshot", Data: full})
			observed, ok := r.session(opened)
			if !ok || app.Data.Find(opened) == nil || observed.Activity.Run == nil || observed.Activity.Run.Status != protocol.ConversationStatusRunning {
				t.Fatal("the inventory did not list the opened session as running")
			}
			if app.Sidebar.Selected != want || app.FocusedConversation() != want || r.pendingSession != "" {
				t.Fatalf("selected %q, focused %q with pending %q after the session appeared; want %q",
					app.Sidebar.Selected, app.FocusedConversation(), r.pendingSession, want)
			}
			f.event(t, "cursor", nil)
			if app.FocusedConversation() != want {
				t.Fatalf("a later update moved the view to %q", app.FocusedConversation())
			}
		})
	}
}
