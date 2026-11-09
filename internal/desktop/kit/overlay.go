package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// Layer is one of the overlay panels an anchor can show.
type Layer uint8

const (
	TipLayer     Layer = iota // a tooltip
	PopoverLayer              // a popover
	CardLayer                 // a hover card
)

// layerKey names an overlay panel by its anchor and layer.
type layerKey struct {
	anchor uint64
	layer  Layer
}

// Anchored keys panel, an overlay panel of anchor, by both. MyGo builds
// every overlay in one layer and names an unkeyed panel by its place
// there, so a tooltip of an anchor built earlier, showing, took the place
// of a popover open below it: the popover replayed its entrance and the
// tooltip drew from its frames. MyGo's menus key their panels by their
// trigger the same way. Call it before the panel's children are built.
func Anchored(panel, anchor ui.Element, l Layer) ui.Element {
	return panel.Key(layerKey{anchor.ID(), l})
}

// layers is the window's record of its open popovers and menus, kept on
// its root (ui.Local, as MenuButton keeps its menus' open states): each
// open state with the frame it last showed in, and how deep the build is
// in an open one's content. MyGo has no frame counter, so frames are told
// apart by their clock, c.Now.
type layers struct {
	now, prev time.Time
	open      map[*bool]time.Time
	inside    int
}

// layersKey keys the window's layers on its root.
type layersKey struct{}

// layersOf returns the window's layers, turned over to this frame.
func layersOf(c *ui.Context) *layers {
	l := ui.Local(c.Root(), layersKey{}, func() layers { return layers{open: map[*bool]time.Time{}} })
	if now := c.Now(); now != l.now {
		l.prev, l.now = l.now, now
	}
	return l
}

// layer notes that the popover or menu open is *open shows this frame
// and builds its content in it: tooltips there still show.
func layer(c *ui.Context, open *bool, content func()) {
	l := layersOf(c)
	l.open[open] = l.now
	l.inside++
	defer func() { l.inside-- }()
	content()
}

// TipsHidden reports whether tooltips and hover cards built here stay
// hidden: a popover or menu is open, this frame or the last (one built
// later in the frame than the tip is known from the last), and the tip is
// not in its content. They would show beneath it, as MyGo paints overlays
// in the order they are built.
func TipsHidden(c *ui.Context) bool {
	l := layersOf(c)
	if l.inside > 0 || len(l.open) == 0 {
		return false
	}
	for open, at := range l.open {
		if !*open || at != l.now && at != l.prev {
			delete(l.open, open)
			continue
		}
		return true
	}
	return false
}
