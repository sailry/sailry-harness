package shell

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
)

// WorkspacePane contributes one kind of main-workspace surface. Callbacks
// operate on the supplied window. Close releases the module's surface;
// the shell removes its tab, split placement and panel. The module's
// window binding releases window-scoped resources on shutdown. Split
// optionally opens a sibling surface and returns its ID.
type WorkspacePane struct {
	Kind   panes.Kind
	Icon   *ui.SVG
	Exists func(*App, string) bool
	Title  func(*App, *ui.Context, string) string
	Render func(*App, *ui.Context, string)
	Close  func(*App, string)
	Split  func(*App, string) (string, bool)
}

func (a *Assembly) RegisterWorkspacePane(pane WorkspacePane) {
	if pane.Kind == "" || pane.Kind == panes.Session || pane.Icon == nil || pane.Exists == nil || pane.Title == nil || pane.Render == nil || pane.Close == nil {
		panic(fmt.Sprintf("shell: workspace pane %q requires a module kind, icon, existence check, title, renderer and close action", pane.Kind))
	}
	if a.HasWorkspacePane(pane.Kind) {
		panic(fmt.Sprintf("shell: duplicate workspace pane %q", pane.Kind))
	}
	if a.workspacePanes == nil {
		a.workspacePanes = make(map[panes.Kind]*WorkspacePane)
	}
	a.workspacePanes[pane.Kind] = &pane
}

func (a Assembly) HasWorkspacePane(kind panes.Kind) bool { return a.workspacePanes[kind] != nil }

func (a *App) workspacePane(kind panes.Kind) *WorkspacePane { return a.workspacePanes[kind] }

func (a *App) HasWorkspacePane(kind panes.Kind) bool { return a.workspacePane(kind) != nil }

// ActiveWorkspacePane identifies the focused module surface on the
// conversation page. A conversation or another page returns a zero target.
func (a *App) ActiveWorkspacePane() panes.Target {
	if t := a.panes.Active; a.Page == ConversationPage && a.HasWorkspacePane(t.Kind) {
		return t
	}
	return panes.Target{}
}
