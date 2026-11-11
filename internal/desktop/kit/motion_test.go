package kit

import (
	"fmt"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// A session keeps a stable mark across apps.
func TestIdentityMatchesReference(t *testing.T) {
	if shape, tint := identity("agent-7"); shape != 2 || tint != 0 {
		t.Fatalf("identity(agent-7) = %d, %d; want 2, 0", shape, tint)
	}
	seen := map[[2]int]bool{}
	for i := range 256 {
		shape, tint := identity(fmt.Sprintf("agent-%d", i))
		seen[[2]int{shape, tint}] = true
	}
	if len(seen) <= 32 {
		t.Fatalf("only %d identities", len(seen))
	}
	// A tile without a glyph shows its name's initial, and its tint alone
	// for a name not given yet.
	t.Run("tile", func(t *testing.T) {
		name := "Alpha"
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			Tile(c, name, nil)
		}, 100, 100)
		tt.Frame()
		if !tt.HasText("A") {
			t.Fatalf("no initial: %q", tt.Texts())
		}
		name = ""
		tt.Frame()
		if tt.HasText("A") || len(tt.Texts()) != 0 {
			t.Errorf("a tile without a name shows %q", tt.Texts())
		}
	})
}

// A row's sweep animates the texts given to it, at FPS, and none with
// Reduce Motion; a row that gives it none asks for no frame.
func TestSweepSharesRow(t *testing.T) {
	give := true
	tt := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		var sw Sweep
		row := ui.Row(c).Gap(8)
		row.Children(func() {
			ui.Text(c, "Read")
			if give {
				sw.Text(c, ui.Text(c, "Read").FontSize(14), ui.Span{Text: "Read", Size: 14})
				sw.Text(c, ui.Text(c, "main.go").Font("monospace"), ui.Span{Text: "main.go", Font: "monospace", Size: 13})
			}
		})
		sw.Frame(row)
	}, 400, 100)
	tt.Frame()
	tt.Frame()
	if !tt.FramePending() {
		t.Fatal("the sweep asks for no frame")
	}
	tt.SetPreferences(ui.Preferences{ReduceMotion: true})
	tt.Frame()
	tt.Frame()
	if tt.FramePending() {
		t.Error("the sweep moves with Reduce Motion")
	}
	tt.SetPreferences(ui.Preferences{})
	give = false
	tt.Frame()
	tt.Frame()
	if tt.FramePending() {
		t.Error("a row without texts in its sweep animates")
	}
}
