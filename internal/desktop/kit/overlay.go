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

// modalKey keys the window's record of a modal dialog on its root.
type modalKey struct{}

// NoteModal notes that a modal dialog is built in this frame, for one built
// outside Kit on Element.Modal. MyGo holds the window's shortcuts behind one
// but does not say that one shows, and the native menu's commands reach the
// app outside its keys.
func NoteModal(c *ui.Context) {
	*ui.Local(c.Root(), modalKey{}, func() bool { return false }) = true
}

// TakeModal reports whether a modal dialog of Kit's (Dialog, BareDialog,
// AlertDialog, ImageViewer) was built since the last call, and clears the
// record. The window calls it once a frame, before it builds its dialogs,
// to hold the commands the native menu sent while one showed.
func TakeModal(c *ui.Context) bool {
	shown := ui.Local(c.Root(), modalKey{}, func() bool { return false })
	was := *shown
	*shown = false
	return was
}

// AlertDialog presents a confirmation through the native window service. The
// first button cancels; headless harnesses use MyGo's in-memory dialog.
func AlertDialog(c *ui.Context, open *bool, title, message string, buttons ...string) int {
	return Confirm(c, open, Confirmation{Title: title, Message: message, Buttons: buttons, Default: 0, Cancel: 0})
}
