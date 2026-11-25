// Package theme gives native pages the current Desktop's semantic foreground.
package theme

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type Mode = theme.Mode

const (
	System = theme.System
	Light  = theme.Light
	Dark   = theme.Dark
)

func Apply(c *ui.Context, id string, mode Mode) { theme.Apply(c, id, mode) }
func Foreground(c *ui.Context) ui.Color         { return theme.Of(c).SurfaceForeground }
