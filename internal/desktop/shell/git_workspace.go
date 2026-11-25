package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
)

type workspaceOverlay struct {
	show  func(*ui.Context) bool
	close func()
}

func (a *App) SetWorkspaceOverlay(show func(*ui.Context) bool, close func()) {
	a.closeWorkspaceOverlay()
	a.workspaceOverlay = workspaceOverlay{show: show, close: close}
}

func (a *App) closeWorkspaceOverlay() {
	if release := a.workspaceOverlay.close; release != nil {
		release()
	}
	a.workspaceOverlay = workspaceOverlay{}
}

func (a *App) renderWorkspaceOverlay(c *ui.Context) {
	if show := a.workspaceOverlay.show; show != nil && !show(c) {
		a.closeWorkspaceOverlay()
	}
}

func (a *App) WorkspaceMount(kind panel.Kind) panel.GitWorkspaceMount {
	if mount := a.Panel.WorkspaceMount(kind); mount != nil {
		return mount
	}
	for _, page := range a.nativePages {
		if page.Resource == kind && page.BindWorkspace != nil {
			return page.BindWorkspace
		}
	}
	return nil
}
