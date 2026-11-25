package conversation

import (
	"strings"
	"testing"
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
