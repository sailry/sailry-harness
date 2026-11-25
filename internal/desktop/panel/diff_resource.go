package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// GitRepository renders the shared read-only change navigator for resource probes.
func (pr *Project) GitRepository(c *ui.Context, _ bool, _ bool, open, reveal func(string)) {
	if len(pr.Data.Changes) == 0 {
		empty(c, icons.Network, "git_no_changes")
		return
	}
	palette := theme.Of(c)
	ui.Scroll(c.Key("changes-list")).Grow(1).Padding(8).Children(func() {
		tree := ui.TreeBase(c)
		tree.Root.Children(func() {
			for _, change := range pr.Data.Changes {
				row := tree.Item(nil).Row.Height(rowHeight).PaddingX(8).Label(baseName(change.Path)).Cursor(ui.CursorPointer)
				if row.Hovered() || pr.Git.Change() == change.Path {
					row.Background(palette.Hover)
				}
				row.Children(func() { ui.Text(c, baseName(change.Path)).FontSize(14).SingleLine() })
				if row.Clicked() {
					open(change.Path)
				}
			}
		})
	})
}

func (pr *Project) GitKeys(*ui.Context, ui.Element) {}
func (pr *Project) GitDialogs(*ui.Context)          {}
