package sidebar

import "github.com/egoist/mygo/ui"

// Dialogs builds the sidebar's dialogs for a page that shows no sidebar, so
// the project editor opened from there shows. View builds them itself.
func Dialogs(c *ui.Context, s *State) { s.dialogs(c) }
