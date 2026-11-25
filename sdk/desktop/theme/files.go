package theme

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func MutedForeground(c *ui.Context) ui.Color { return theme.Of(c).MutedForeground }
