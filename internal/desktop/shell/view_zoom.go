package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// The view zoom's commands, for the window's keys and the native menu (Do).
const (
	ViewZoomIn    = "view.zoom_in"
	ViewZoomOut   = "view.zoom_out"
	ViewZoomReset = "view.zoom_reset"
)

// viewZoomKeys are the zoom's fixed keys, Cmd standing for Ctrl off macOS:
// Cmd+= and Cmd+Shift+= (Cmd++) zoom in, Cmd+- out and Cmd+0 resets.
var viewZoomKeys = []Shortcut{
	{ID: ViewZoomIn, Mods: ui.Cmd, Key: ui.KeyEqual},
	{ID: ViewZoomIn, Mods: ui.Cmd | ui.Shift, Key: ui.KeyEqual},
	{ID: ViewZoomOut, Mods: ui.Cmd, Key: ui.KeyMinus},
	{ID: ViewZoomReset, Mods: ui.Cmd, Key: ui.Key0},
}

// viewZoom is what the zoom commands keep beyond the settings: the text
// size steps In added past the largest interface size, which Out takes back
// before it lowers the interface size. A text size changed elsewhere, on
// the View page or in another window, ends them.
type viewZoom struct {
	extra int
	text  float64
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

// runViewZoom carries out a zoom command, false for another command. In
// steps the interface size up to its largest, then the conversation's text
// size; Out undoes those text steps first, then the interface size; Reset
// returns both to their defaults.
func (a *App) runViewZoom(id string) bool {
	m, z := a.Settings.Model, &a.viewZoom
	if m.TextSize != z.text {
		z.extra = 0
	}
	level, last := viewZoomIndex(m.ViewZoom), len(preferences.ViewZoomLevels)-1
	switch id {
	case ViewZoomIn:
		switch {
		case level < last:
			level++
		case m.TextSize < settings.MaxTextSize:
			m.TextSize = min(m.TextSize+1, settings.MaxTextSize)
			z.extra++
		}
	case ViewZoomOut:
		switch {
		case z.extra > 0 && m.TextSize > settings.MinTextSize:
			m.TextSize = max(m.TextSize-1, settings.MinTextSize)
			z.extra--
		case level > 0:
			level--
		}
	case ViewZoomReset:
		level, m.TextSize, z.extra = 0, settings.DefaultTextSize, 0
	default:
		return false
	}
	m.ViewZoom, z.text = level, m.TextSize
	return true
}

// viewZoomIndex is an interface size as an index of the levels, the first
// (100%) for an unknown one.
func viewZoomIndex(i int) int {
	if i < 0 || i >= len(preferences.ViewZoomLevels) {
		return 0
	}
	return i
}

// viewZoomFactor is the interface size as a zoom, 1 for 100%.
func (a *App) viewZoomFactor() float32 {
	return float32(preferences.ViewZoomLevels[viewZoomIndex(a.Settings.ViewZoom)]) / 100
}

// applyViewZoom zooms the window's content, which MyGo lays out, draws and
// maps input at (Context.SetZoom); a change builds the frame again at the
// new size. The shell's columns keep their widths in the window's DIPs
// (frame), and the conversation's text size scales inside the zoom.
func (a *App) applyViewZoom(c *ui.Context) {
	c.SetZoom(a.viewZoomFactor())
}
