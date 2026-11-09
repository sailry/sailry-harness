package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// effects returns the theme's effect layers of a surface; none yet.
func effects(c *ui.Context, v *theme.Variant, surface string) []theme.Effect { return nil }

// paintEffects paints effect layers into r.
func paintEffects(pt *ui.Painter, r ui.Rect, layers []theme.Effect) {}
