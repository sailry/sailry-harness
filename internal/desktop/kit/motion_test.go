package kit

import (
	"fmt"
	"testing"
	"time"

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

	t.Run("level with the icon", testLevel)

	// The band lights only the pixels the still text has: its glyphs sit
	// exactly on the text's, in another font and at every text size of a
	// row (below, at and above the theme's 14, the strut of the band's
	// line), wherever the row lands on the pixel grid. The label is a
	// RichText of its span, as Text asks; a plain text agrees with the
	// band at the theme's own size.
	t.Run("band on the glyphs", func(t *testing.T) {
		label, detail := "Search", `"--muted-foreground" in src/styles/`
		for _, tc := range []struct {
			size  float32
			plain bool
		}{{12, false}, {14, false}, {14, true}, {18, false}} {
			size := tc.size
			for _, scale := range []float32{1, 1.5, 2} {
				for _, pad := range []float32{20, 20.2, 20.45, 20.7, 20.95} {
					tt := ui.NewTester(func(c *ui.Context) {
						theme.Apply(c, "", theme.Dark)
						p := theme.Of(c)
						ui.Column(c).Fill().Background(p.Background).Padding(pad).Children(func() {
							var sw Sweep
							row := ui.Row(c).Gap(8)
							row.Children(func() {
								text := ui.Span{Text: label, Size: size, Color: p.MutedForeground}
								if tc.plain {
									sw.Text(c, ui.Text(c, label).FontSize(size).SingleLine().Shrink(0).TextColor(p.MutedForeground), text)
								} else {
									sw.Text(c, ui.RichText(c, text).SingleLine().Shrink(0), text)
								}
								span := ui.Span{Text: detail, Font: "monospace", Size: size - 1, Color: p.MutedForeground.Alpha(0.65)}
								sw.Text(c, ui.RichText(c, span).SingleLine().Grow(1).Shrink(1).MinWidth(0), span)
							})
							sw.Frame(row)
						})
					}, 700, 80)
					tt.SetScale(scale)
					tt.SetPreferences(ui.Preferences{ReduceMotion: true})
					tt.Frame()
					tt.Frame()
					still := tt.Image()
					pix := append([]byte(nil), still.Pix...)
					tt.SetPreferences(ui.Preferences{})
					lit, spill := 0, 0
					for range 40 {
						tt.Advance(50 * time.Millisecond)
						tt.Frame()
						now := tt.Image().Pix
						for o := 0; o+3 < len(pix); o += 4 {
							if now[o] == pix[o] && now[o+1] == pix[o+1] && now[o+2] == pix[o+2] {
								continue
							}
							lit++
							if pix[o] == pix[0] && pix[o+1] == pix[1] && pix[o+2] == pix[2] {
								spill++
							}
						}
					}
					if lit == 0 || spill != 0 {
						t.Errorf("size %v (plain text %v), scale %v, inset %v: the band lit %d pixels, %d of them beside the text's glyphs", size, tc.plain, scale, pad, lit, spill)
					}
				}
			}
		}
	})
}
