package controls

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

func Ghost(c *ui.Context, label string, selected bool) ui.Element {
	return kit.Ghost(c, label, selected)
}

func IconButton(c *ui.Context, icon *ui.SVG, label string, size float32, selected bool) ui.Element {
	return kit.IconButton(c, icon, label, size, selected)
}

func FileTreeBusy(c *ui.Context) { kit.Busy(c) }
