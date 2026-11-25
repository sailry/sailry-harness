package controls

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type NavigationTab = kit.NavigationTab
type NavigationTabsProps = kit.NavigationTabsProps

func NavigationTabs(c *ui.Context, tabs []NavigationTab, props NavigationTabsProps) ui.Element {
	return kit.NavigationTabs(c, tabs, props)
}
