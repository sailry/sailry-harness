package kit

import "github.com/egoist/mygo/ui"

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
