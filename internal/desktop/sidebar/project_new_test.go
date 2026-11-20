package sidebar

import (
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Hovering a project row shows a "+" that opens a new conversation in that
// project; clicking it leaves the project open rather than toggling it.
func TestProjectRowNewSession(t *testing.T) {
	d := preview.Demo("en", "local")
	s := New(d, preview.DemoStart)
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Children(func() { View(c, s) })
	}, 280, 900)
	tt.SetLocale("en")
	settle := func() {
		tt.Frame()
		tt.Advance(time.Second)
		tt.Frame()
	}
	settle()
	var pr *preview.Project
	for _, candidate := range s.projects() {
		if *s.opened("project/" + candidate.ID) {
			pr = candidate
			break
		}
	}
	if pr == nil {
		t.Fatal("the demo has no open project")
	}
	row, ok := tt.Access(pr.Name)
	if !ok {
		t.Fatalf("project %q is not shown", pr.Name)
	}
	tt.Move(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
	settle()
	// The top button shares the label; this one sits before the chevron:
	// the row's end padding, the chevron, the gap, then the button's centre.
	tt.ClickAt(row.Bounds.X+row.Bounds.W-8-14-8-10, row.Bounds.Y+row.Bounds.H/2)
	settle()
	if !strings.HasPrefix(s.Selected, pr.ID+"/new-") || !*s.opened("project/" + pr.ID) {
		t.Fatalf("selected %q, project open %v", s.Selected, *s.opened("project/" + pr.ID))
	}
}
