package sidebar

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestConfirmDeleteKeepsShownHostAndRevision(t *testing.T) {
	row := &viewmodel.Conversation{ID: "shared", Node: "first", Revision: 5, Title: "First host session"}
	s := New(&viewmodel.Data{Unassigned: []*viewmodel.Conversation{row}}, row.ID)
	var removed *viewmodel.Conversation
	s.Actions = &Actions{Delete: func(cv *viewmodel.Conversation) { removed = cv }}
	window := ui.NewTester(func(c *ui.Context) { s.confirmDelete(c, row) }, 300, 200)
	window.Frame()
	row.Node, row.Revision = "second", 7
	s.confirm.run()
	if removed == nil || removed.ID != "shared" || removed.Node != "first" || removed.Revision != 5 {
		t.Fatalf("confirmation changed its target: %+v", removed)
	}
}
