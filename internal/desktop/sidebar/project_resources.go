package sidebar

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ProjectOpen func(project *viewmodel.Project, worktreePath string)

// ProjectResourceAction contributes a single action to both the sidebar
// context menu and the overview toolbar, without owning their layout.
type ProjectResourceAction struct {
	Label string
	Icon  *ui.SVG
	Order int
	Open  ProjectOpen
}

func (a ProjectResourceAction) Menu(c *ui.Context, m *ui.Menu, project *viewmodel.Project) {
	if m.Item(i18n.T(c, a.Label)).Disabled(project.Offline || a.Open == nil).Chosen() && a.Open != nil {
		a.Open(project, "")
	}
}
