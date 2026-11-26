package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

func TestModuleOwnsSplitContent(t *testing.T) {
	const kind panes.Kind = "notes"
	target := panes.Target{Kind: kind, ID: preview.Big}
	open, closed := true, 0
	var assembly Assembly
	assembly.RegisterWorkspacePane(WorkspacePane{
		Kind: kind, Icon: icons.Folder,
		Exists: func(_ *App, id string) bool { return open && id == target.ID },
		Title:  func(_ *App, _ *ui.Context, _ string) string { return "Project notes" },
		Render: func(_ *App, c *ui.Context, id string) { ui.Text(c, "Notes for "+id) },
		Close:  func(_ *App, _ string) { closed++; open = false },
	})
	app := NewWithAssembly(preview.Load(), assembly)
	if !app.OpenWorkspacePane(kind, target.ID) {
		t.Fatal("module could not open its surface")
	}
	app.KeepWorkspacePane(kind, target.ID)
	app.OpenConversation(preview.Big)
	tt := uitest.Fast(app.View, 1280, 820, "en")
	tt.Frame()
	tab, ok := headerButton(tt, "Project notes")
	if !ok {
		t.Fatal("module tab is missing")
	}
	dragTo(t, tt, tab, 1150, 400, nil)
	if !slices.Equal(app.panes.Members(target), []panes.Target{session(preview.Big), target}) ||
		!tt.HasText("Notes for "+target.ID) || app.FocusedConversation() != "" {
		t.Fatalf("module split: active %v, members %v", app.panes.Active, app.panes.Members(target))
	}
	if owner, ok := panePanelTarget(app.panelOwner()); !ok || owner != target {
		t.Fatalf("module panel owner = %v", owner)
	}
	app.CloseWorkspacePane(target.Kind, target.ID)
	tt.Frame()
	if closed != 1 || app.panes.Active != session(preview.Big) || app.panes.Displayed() != nil || slices.Contains(app.panes.Kept, target) {
		t.Fatalf("closing module: closed %d, active %v, tabs %v", closed, app.panes.Active, app.panes.Kept)
	}
}
