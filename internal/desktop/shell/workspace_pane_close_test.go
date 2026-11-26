package shell

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// A module surface closed by ⌘W, alone or in a split, receives no Render or
// Title call for its released ID in the build that closed it.
func TestWorkspacePaneCloseStopsCallbacks(t *testing.T) {
	for _, split := range []bool{false, true} {
		name := "alone"
		if split {
			name = "split"
		}
		t.Run(name, func(t *testing.T) {
			const kind = panes.Kind("close-fixture")
			const id = "surface-1"
			open := map[string]bool{id: true}
			late := 0
			assembly := TestHarnessAssembly()
			assembly.RegisterWorkspacePane(WorkspacePane{
				Kind: kind, Icon: icons.Terminal,
				Exists: func(_ *App, id string) bool { return open[id] },
				Title: func(_ *App, _ *ui.Context, id string) string {
					if !open[id] {
						late++
					}
					return "Fixture " + id
				},
				Render: func(_ *App, c *ui.Context, id string) {
					if !open[id] {
						late++
					}
					ui.Text(c, "surface body "+id)
				},
				Close: func(_ *App, id string) { delete(open, id) },
			})
			a := NewWithAssembly(preview.Load(), assembly)
			tt := ui.NewTesterWith(a.View, 1280, 820, opaque)
			tt.SetLocale("en")
			tt.Frame()
			target := panes.Target{Kind: kind, ID: id}
			if !a.OpenWorkspacePane(kind, id) {
				t.Fatal("the fixture surface did not open")
			}
			if split && !a.panes.Drop(session(a.Sidebar.Selected), target, panes.Right) {
				t.Fatal("the fixture surface did not split beside the conversation")
			}
			tt.Frame()
			r, ok := tt.Find("surface body " + id)
			if !ok {
				t.Fatalf("the surface did not render: %q", tt.Texts())
			}
			tt.ClickAt(r.X+r.W/2, r.Y+r.H/2)
			tt.Frame()
			if a.panes.Active != target {
				t.Fatalf("active pane = %v, want %v", a.panes.Active, target)
			}
			tt.Key(CloseFocused.Mods, CloseFocused.Key)
			tt.Frame()
			if open[id] || a.paneExists(target) {
				t.Fatal("⌘W did not close the surface")
			}
			if late != 0 {
				t.Fatalf("closed surface received %d Render or Title calls", late)
			}
		})
	}
}
