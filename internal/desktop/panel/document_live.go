package panel

import "github.com/egoist/mygo/ui"

// SetDocumentView gives the renderer of a session document's HTML, which
// reports false while it has nothing to show.
func (s *State) SetDocumentView(view func(*ui.Context, string) bool) { s.documentView = view }

// liveDocument renders a document tab's own HTML in place of its text.
func (s *State) liveDocument(c *ui.Context, id string) bool {
	if !s.Live || s.documentView == nil {
		return false
	}
	rendered := false
	ui.Column(c.Key(id)).Grow(1).MinHeight(0).Children(func() { rendered = s.documentView(c, id) })
	return rendered
}
