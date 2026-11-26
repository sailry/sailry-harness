package conversation

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestLetteringCoversPrintableASCII(t *testing.T) {
	for c := byte(' '); c <= '~'; c++ {
		g, ok := glyphs[c]
		if !ok {
			t.Fatalf("no glyph for %q", c)
		}
		if g.w < 1 || g.w > 5 {
			t.Fatalf("glyph %q is %d cells wide", c, g.w)
		}
		lit := false
		for _, row := range g.rows {
			lit = lit || row != 0
		}
		if lit == (c == ' ') {
			t.Fatalf("glyph %q lit is %v", c, lit)
		}
	}
	if len(glyphs) != '~'-' '+1 {
		t.Fatalf("the face has %d glyphs, want printable ASCII", len(glyphs))
	}
}

func TestLetteringGlyphsAreDistinct(t *testing.T) {
	seen := map[glyph]byte{}
	for c := byte('!'); c <= '~'; c++ {
		if other, ok := seen[glyphs[c]]; ok {
			t.Fatalf("%q draws the same as %q", c, other)
		}
		seen[glyphs[c]] = c
	}
	// Lowercase sits on the x-height: nothing above row 2 but ascenders
	// and the dots of i and j.
	for c := byte('a'); c <= 'z'; c++ {
		top := glyphs[c].rows[0] | glyphs[c].rows[1]
		if top != 0 && !strings.ContainsRune("bdfhijklt", rune(c)) {
			t.Fatalf("%q reaches above the x-height", c)
		}
	}
	for _, c := range []byte("gjpqy") {
		if glyphs[c].rows[7]|glyphs[c].rows[8] == 0 {
			t.Fatalf("%q has no descender", c)
		}
	}
}

func TestLetteringWraps(t *testing.T) {
	text := "Hello, World!"
	one := layoutLettering(text, 0)
	if one.h != 8*letterLine || one.w != float32(16*cellsOf(text)) {
		t.Fatalf("one line is %v x %v", one.w, one.h)
	}
	cols := max(cellsOf("Hello,"), cellsOf("World!"))
	lines := wrapText(text, cols)
	if strings.Join(lines, "|") != "Hello,|World!" {
		t.Fatalf("wrapped as %q", lines)
	}
	two := layoutLettering(text, cols)
	if two.h != 8*(2*letterLine+letterGap) || two.w > float32(16*cols) {
		t.Fatalf("two lines are %v x %v", two.w, two.h)
	}
	if len(two.particles) != len(one.particles) {
		t.Fatalf("wrapping drew %d particles, one line %d", len(two.particles), len(one.particles))
	}
	// A word longer than a line breaks inside it, a glyph a line at least.
	for _, line := range wrapText("Supercalifragilistic", 1) {
		if len(line) != 1 {
			t.Fatalf("a line of one cell holds %q", line)
		}
	}
	for _, line := range wrapText("Supercalifragilistic", 17) {
		if cellsOf(line) > 17 {
			t.Fatalf("%q overflows 17 cells", line)
		}
	}
	for _, q := range two.particles {
		if q[0] < 0 || q[1] < 0 || q[0]+dot > two.w || q[1]+dot > two.h {
			t.Fatalf("particle %v outside %v x %v", q, two.w, two.h)
		}
	}
}

func TestWelcomeDefaultMarkPrecedenceAndLayout(t *testing.T) {
	t.Run("precedence between custom Mark, DefaultMark, and DefaultWordmark", func(t *testing.T) {
		state := &State{}
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Dark)
			Welcome(c, state, func() {}, func(string) {})
		}, 900, 700)
		tt.SetLocale("en")
		tt.Frame()
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleImage, DefaultWordmark); !ok {
			t.Fatalf("expected %q fallback when both Mark and DefaultMark are blank", DefaultWordmark)
		}

		state.DefaultMark = "SAILRY HARNESS"
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleImage, "SAILRY HARNESS"); !ok {
			t.Fatalf("expected DefaultMark when Mark is blank")
		}

		state.Mark = "CUSTOM MARK"
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleImage, "CUSTOM MARK"); !ok {
			t.Fatalf("expected custom Mark to override DefaultMark")
		}

		state.Mark = "   "
		tt.Frame()
		if _, ok := tt.AccessByRole(ui.RoleImage, "SAILRY HARNESS"); !ok {
			t.Fatalf("expected clearing Mark to revert to DefaultMark")
		}
	})

	t.Run("product default marks fit desktop and compact widths without overflow", func(t *testing.T) {
		for _, tc := range []struct {
			mark    string
			width   int
			wrapped bool
		}{
			{"SAILRY", 1100, false},
			{"SAILRY", 360, false},
			{"SAILRY HARNESS", 1100, false},
			{"SAILRY HARNESS", 700, false},
			{"SAILRY HARNESS", 360, true},
			{"SAILRY HARNESS", 300, true},
			{"SAILRY CODE", 1100, false},
			{"SAILRY CODE", 700, false},
			{"SAILRY CODE", 360, true},
			{"SAILRY CODE", 300, false},
		} {
			state := &State{DefaultMark: tc.mark}
			tt := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				Welcome(c, state, func() {}, func(string) {})
			}, tc.width, 700)
			tt.SetLocale("en")
			tt.Frame()
			tt.Frame()
			info, ok := tt.AccessByRole(ui.RoleImage, tc.mark)
			if !ok {
				t.Fatalf("%s @ %d: missing welcome lettering", tc.mark, tc.width)
			}
			r := info.Bounds
			if r.X < 0 || r.X+r.W > float32(tc.width) || r.W <= 0 || r.H <= 0 {
				t.Fatalf("%s @ %d: lettering bounds %+v overflow width", tc.mark, tc.width, r)
			}
			// Compare rendered line count via laid-out box height.
			wrapped := state.letters.h > 8*letterLine
			if wrapped != tc.wrapped {
				t.Errorf("%s @ %d: wrapped = %v (box %v x %v), want %v", tc.mark, tc.width, wrapped, r.W, r.H, tc.wrapped)
			}
		}
	})
}
