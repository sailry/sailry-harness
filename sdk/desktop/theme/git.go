package theme

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type Palette = theme.Palette

func Of(c *ui.Context) *Palette { return theme.Of(c) }
