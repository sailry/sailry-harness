package sidebar

import (
	"cmp"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// ProjectMenuAction registers entries in the project's native context menu and
// in the two slots around Copy path in the overview's More menu.
type ProjectMenuAction struct {
	Order      int
	Context    func(*ui.Context, *ui.Menu, *viewmodel.Project)
	BeforeCopy func(*ui.Context, *kit.Menu, *viewmodel.Project)
	AfterCopy  func(*ui.Context, *kit.Menu, *viewmodel.Project)
}

func (a *Actions) RegisterProjectMenu(action ProjectMenuAction) {
	a.ProjectMenus = append(a.ProjectMenus, action)
	slices.SortStableFunc(a.ProjectMenus, func(first, second ProjectMenuAction) int { return cmp.Compare(first.Order, second.Order) })
}

func sampleWorktreeProjectMenu(run func(*ui.Context, *viewmodel.Project, string, string)) ProjectMenuAction {
	return ProjectMenuAction{
		Order: 30,
		Context: func(c *ui.Context, m *ui.Menu, pr *viewmodel.Project) {
			t := func(key string) string { return i18n.T(c, key) }
			m.Submenu(t("worktree_manage"), func(m *ui.Menu) {
				if m.Item(t("worktree_create")).Chosen() {
					run(c, pr, "create", "")
				}
				m.Separator()
				for _, w := range pr.Worktrees {
					if m.Item(w.Branch).Checked(w.Main).Chosen() {
						run(c, pr, "select", w.Path)
					}
				}
				if len(pr.Worktrees) < 2 {
					return
				}
				m.Separator()
				if !kit.Demo(c) {
					if m.Item(t("worktree_remove")).Chosen() {
						run(c, pr, "remove", "")
					}
					return
				}
				m.Submenu(t("worktree_remove"), func(m *ui.Menu) {
					for _, w := range pr.Worktrees {
						if !w.Main && m.Item(w.Branch).Chosen() {
							run(c, pr, "remove", w.Path)
						}
					}
				})
			})
		},
		BeforeCopy: func(c *ui.Context, m *kit.Menu, pr *viewmodel.Project) {
			if m.Item(i18n.T(c, "worktree_create")).Icon(icons.Plus).Disabled(pr.Offline).Chosen() {
				run(c, pr, "create", "")
			}
		},
		AfterCopy: func(c *ui.Context, m *kit.Menu, pr *viewmodel.Project) {
			if m.Item(i18n.T(c, "worktree_remove")).Icon(icons.CircleX).Disabled(pr.Offline || len(pr.Worktrees) < 2).Chosen() {
				run(c, pr, "remove", "")
			}
		},
	}
}
