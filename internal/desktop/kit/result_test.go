package kit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// Result and Details take the caller's key for their root: two under one
// parent, each keyed by its caller, are two elements.
func TestCallerKeys(t *testing.T) {
	first, second := true, true
	tt := ui.NewTesterWith(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			Result(c.Key("failed"), ResultProps{Status: ResultError, Title: "Could not read"})
			Result(c.Key("empty"), ResultProps{Title: "Nothing here"})
			Details(c.Key("first"), &first, "First details", "", "one")
			Details(c.Key("second"), &second, "Second details", "", "two")
		})
	}, 800, 600, ui.TesterOptions{})
	tt.Frame()
	tt.Frame()
	failed, shown := tt.Find("Could not read")
	empty, also := tt.Find("Nothing here")
	if !shown || !also || failed.Y+failed.H > empty.Y {
		t.Fatalf("the two results are at %+v (%v) and %+v (%v)", failed, shown, empty, also)
	}
	for _, title := range []string{"First details", "Second details"} {
		if _, shown := tt.Find(title); !shown {
			t.Errorf("no dialog titled %q: %q", title, tt.Texts())
		}
	}
}
