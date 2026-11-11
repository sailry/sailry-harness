package composer_test

import (
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A chosen file of the attachment picker is told by the neutral selected
// fill, as the folder picker's entries are, not by the emphasis color.
func TestFileSelectionIsNeutral(t *testing.T) {
	root := &composer.Entry{Name: "work", Path: "/work", Children: []*composer.Entry{
		{Name: "notes.txt", Path: "/work/notes.txt", Mime: "text/plain", Size: 12},
		{Name: "plan.md", Path: "/work/plan.md", Mime: "text/markdown", Size: 34},
	}}
	state := composer.New(preview.DemoModel)
	ctx := &composer.Context{Files: root, Activity: composer.Activity{Connected: true}}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, theme.BuiltinID, theme.Light) // the application's theme, which the neutral one is made of
		ui.Column(c).Fill().Justify(ui.End).Children(func() { composer.View(c, state, ctx, &owner{}) })
	}, 760, 640, ui.TesterOptions{})
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	settle := func() {
		for range 3 {
			tt.Frame()
			tt.Advance(400_000_000)
		}
		tt.Frame()
	}
	attach, ok := tt.AccessByRole(ui.RoleButton, "Add attachment")
	if !ok {
		t.Fatalf("no attach button: %q", tt.Texts())
	}
	tt.ClickAt(attach.Bounds.X+attach.Bounds.W/2, attach.Bounds.Y+attach.Bounds.H/2)
	settle()
	chosen, shown := tt.Find("notes.txt")
	other, also := tt.Find("plan.md")
	if !shown || !also {
		t.Fatalf("the picker lists no files: %q", tt.Texts())
	}
	tt.ClickAt(chosen.X+chosen.W/2, chosen.Y+chosen.H/2)
	tt.Move(2, 2)
	settle()
	// A point at each row's end, past its cells' text, where only the row's
	// fill is drawn.
	at := func(row ui.Rect) [3]int {
		c := tt.Image().RGBAAt(int(row.X+row.W-20), int(row.Y+row.H/2))
		return [3]int{int(c.R), int(c.G), int(c.B)}
	}
	fill, ground := at(chosen), at(other)
	if dir := os.Getenv("SAILRY_SNAPSHOTS"); dir != "" {
		out, _ := os.Create(filepath.Join(dir, "composer-file-selection.png"))
		png.Encode(out, tt.Image())
		out.Close()
	}
	if fill == ground {
		t.Fatalf("the chosen file's row is not filled: %v", fill)
	}
	d := [3]int{fill[0] - ground[0], fill[1] - ground[1], fill[2] - ground[2]}
	if max(d[0], d[1], d[2])-min(d[0], d[1], d[2]) > 3 {
		t.Fatalf("the chosen file's row is filled %v over %v, want a neutral gray", fill, ground)
	}
}
