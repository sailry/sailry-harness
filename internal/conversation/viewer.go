package conversation

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/kit"
)

// Zoom limits of the image viewer (content/images/preview/viewport.rs).
const (
	MinZoom = kit.MinZoom
	MaxZoom = kit.MaxZoom
)

// Viewer is the image viewer's state (content/images/preview.rs) over a
// message's pictures: Kit's Viewer, whose methods zoom, pan, turn, flip
// and step it.
type Viewer = kit.Viewer[Image]

// viewer draws the image viewer over the window while open (kit's
// ImageViewer). Copy, Save and Open go to the shell.
func (s *State) viewer(c *ui.Context, actions Actions) {
	kit.ImageViewer(c, &s.view, func(im Image) kit.ViewerItem {
		return kit.ViewerItem{Source: im.Source, Name: im.Name}
	}, actions.ImageAction)
}
