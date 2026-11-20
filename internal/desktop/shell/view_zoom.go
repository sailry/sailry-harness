package shell

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
)

// The view zoom's commands, for the window's keys and the native menu (Do).
const (
	ViewZoomIn    = "view.zoom_in"
	ViewZoomOut   = "view.zoom_out"
	ViewZoomReset = "view.zoom_reset"
)

// viewZoomKeys are the zoom's fixed keys, ⌘ standing for Ctrl off macOS:
// ⌘= and ⌘+ (⌘⇧=) zoom in, ⌘- out and ⌘0 back to 100%, as in a browser.
var viewZoomKeys = []Shortcut{
	{ID: ViewZoomIn, Mods: ui.Cmd, Key: ui.KeyEqual},
	{ID: ViewZoomIn, Mods: ui.Cmd | ui.Shift, Key: ui.KeyEqual},
	{ID: ViewZoomOut, Mods: ui.Cmd, Key: ui.KeyMinus},
	{ID: ViewZoomReset, Mods: ui.Cmd, Key: ui.Key0},
}

// viewZoomShortcuts runs the zoom's keys as the window's (shortcuts): a
// focused element that takes one, such as the image viewer, keeps it, and
// a modal dialog holds them. A held key steps once.
func (a *App) viewZoomShortcuts(c *ui.Context) {
	for _, s := range viewZoomKeys {
		if c.Shortcut(s.Mods, s.Key) && !c.ShortcutRepeated() {
			a.runViewZoom(s.ID)
		}
	}
}

// runViewZoom carries out a zoom command, false for another command.
func (a *App) runViewZoom(id string) bool {
	levels := preferences.ViewZoomLevels
	i := slices.Index(levels, viewZoomLevel(a.ViewZoom))
	switch id {
	case ViewZoomIn:
		i = min(i+1, len(levels)-1)
	case ViewZoomOut:
		i = max(i-1, 0)
	case ViewZoomReset:
		i = slices.Index(levels, 100)
	default:
		return false
	}
	a.ViewZoom = levels[i]
	return true
}

// viewZoomLevel is a zoom in percent as one of the levels, 100 for none or
// an unknown one.
func viewZoomLevel(percent uint16) uint16 {
	if slices.Contains(preferences.ViewZoomLevels, percent) {
		return percent
	}
	return 100
}

// applyViewZoom zooms the window's content, which MyGo lays out, draws and
// maps input at (Context.SetZoom); a change builds the frame again at the
// new size. The conversation's text size scales inside it on its own.
func (a *App) applyViewZoom(c *ui.Context) {
	c.SetZoom(float32(viewZoomLevel(a.ViewZoom)) / 100)
}
