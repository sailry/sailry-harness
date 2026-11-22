package panel

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestGitProjectionRefreshesTextAndInPlaceStatus(t *testing.T) {
	file := viewmodel.FileChange{Path: "a", Unstaged: "M", Hunks: []viewmodel.Hunk{{Lines: []viewmodel.DiffLine{{Kind: viewmodel.Added, Text: "first"}}}}}
	var shown string
	var row *changeRow
	tt := ui.NewTester(func(c *ui.Context) {
		box := ui.Column(c.Key("file"))
		_, shown = gitDiffTexts(box, file)
		rows := changeListRows(box, []viewmodel.FileChange{file}, changeNav{grouping: "none"})
		if len(rows) != 1 {
			t.Fatal("expected one status row")
		}
		row = rows[0]
	}, 200, 200)
	tt.Frame()
	first := row
	if shown != "first\n" || row.entry.Hunks != nil {
		t.Fatal("initial diff or sidebar projection is incorrect")
	}
	file.Hunks = []viewmodel.Hunk{{Lines: []viewmodel.DiffLine{{Kind: viewmodel.Added, Text: "replacement"}}}}
	tt.Frame()
	if shown != "replacement\n" || row != first {
		t.Fatal("replacing hunks should update the editor, not rebuild status rows")
	}
	file.Unstaged = "D"
	tt.Frame()
	if row.entry.Unstaged != "D" {
		t.Fatal("in-place status update was not shown")
	}
	file.Hunks = nil
	tt.Frame()
	if shown != "\n" {
		t.Fatal("empty diff retained the old editor input")
	}
}
