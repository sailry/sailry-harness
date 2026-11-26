package conversation

import "strings"

// The welcome page's square-dot lettering face for printable ASCII. A glyph is
// a grid of cells, 7 of cap height and 2 of descender; each cell is drawn as
// 2 x 2 square particles so strokes are two particles thick.

// glyphRows are a glyph's cell rows: caps and ascenders in rows 0-6, the
// x-height in 2-6, descenders in 7-8.
const glyphRows = 9

// Lettering metrics in particles: a line is the glyph rows, a particle of
// margin above and below; wrapped lines are letterGap apart.
const (
	letterLine = 2*glyphRows + 2
	letterGap  = 4
)

// glyph is a glyph's width in cells and its rows, bit x set where cell x is
// on.
type glyph struct {
	w    int
	rows [glyphRows]uint8
}

// glyphTable is the face in banks: a line of the bank's characters, then
// glyphRows lines of their cells, '#' on and '.' off, glyphs apart by a
// space. The backtick, which a raw string cannot hold, is appended.
const glyphTable = `ABCDEFGH
##### ####. ##### ####. ##### ##### ##### #...#
#...# #...# #.... #...# #.... #.... #.... #...#
#...# #...# #.... #...# #.... #.... #.... #...#
##### ##### #.... #...# ####. ####. #.### #####
#...# #...# #.... #...# #.... #.... #...# #...#
#...# #...# #.... #...# #.... #.... #...# #...#
#...# ##### ##### ####. ##### #.... ##### #...#
..... ..... ..... ..... ..... ..... ..... .....
..... ..... ..... ..... ..... ..... ..... .....

IJKLMNOP
### ....# #...# #.... #...# #...# ##### #####
.#. ....# #..#. #.... ##.## ##..# #...# #...#
.#. ....# #.#.. #.... #.#.# #.#.# #...# #...#
.#. ....# ##... #.... #.#.# #..## #...# #####
.#. ....# #.#.. #.... #...# #...# #...# #....
.#. #...# #..#. #.... #...# #...# #...# #....
### ##### #...# ##### #...# #...# ##### #....
... ..... ..... ..... ..... ..... ..... .....
... ..... ..... ..... ..... ..... ..... .....

QRSTUVWX
##### ##### ##### ##### #...# #...# #...# #...#
#...# #...# #.... ..#.. #...# #...# #...# #...#
#...# #...# #.... ..#.. #...# #...# #...# .#.#.
#...# ##### ##### ..#.. #...# #...# #.#.# ..#..
#.#.# #.#.. ....# ..#.. #...# .#.#. #.#.# .#.#.
#..#. #..#. ....# ..#.. #...# .#.#. ##.## #...#
###.# #...# ##### ..#.. ##### ..#.. #...# #...#
..... ..... ..... ..... ..... ..... ..... .....
..... ..... ..... ..... ..... ..... ..... .....

YZabcdef
#...# ##### ..... #.... .... ....# ..... .###
#...# ....# ..... #.... .... ....# ..... .#..
.#.#. ...#. ##### ##### #### ##### ##### ####
..#.. ..#.. ....# #...# #... #...# #...# .#..
..#.. .#... ##### #...# #... #...# ##### .#..
..#.. #.... #...# #...# #... #...# #.... .#..
..#.. ##### ##### ##### #### ##### ##### .#..
..... ..... ..... ..... .... ..... ..... ....
..... ..... ..... ..... .... ..... ..... ....

ghijklmn
..... #.... # .# #... #. ..... .....
..... #.... . .. #... #. ..... .....
##### ##### # .# #..# #. ##### #####
#...# #...# # .# #.#. #. #.#.# #...#
#...# #...# # .# ##.. #. #.#.# #...#
#...# #...# # .# #.#. #. #.#.# #...#
##### #...# # .# #..# ## #.#.# #...#
....# ..... . .# .... .. ..... .....
##### ..... . ## .... .. ..... .....

opqrstuv
..... ..... ..... .... ..... .#.. ..... .....
..... ..... ..... .... ..... .#.. ..... .....
##### ##### ##### #.## ##### #### #...# #...#
#...# #...# #...# ##.. #.... .#.. #...# #...#
#...# #...# #...# #... ##### .#.. #...# #...#
#...# #...# #...# #... ....# .#.. #...# .#.#.
##### ##### ##### #... ##### .### ##### ..#..
..... #.... ....# .... ..... .... ..... .....
..... #.... ....# .... ..... .... ..... .....

wxyz0123
..... ..... ..... ..... ##### .#. ##### #####
..... ..... ..... ..... #...# ##. ....# ....#
#...# #...# #...# ##### #..## .#. ....# ....#
#...# .#.#. #...# ...#. #.#.# .#. ##### .####
#.#.# ..#.. #...# ..#.. ##..# .#. #.... ....#
#.#.# .#.#. #...# .#... #...# .#. #.... ....#
##### #...# ##### ##### ##### ### ##### #####
..... ..... ....# ..... ..... ... ..... .....
..... ..... ##### ..... ..... ... ..... .....

456789.,
#...# ##### ##### ##### .###. ##### . ..
#...# #.... #.... ....# #...# #...# . ..
#...# #.... #.... ....# #...# #...# . ..
##### ####. ##### ...#. .###. ##### . ..
....# ....# #...# ..#.. #...# ....# . ..
....# ....# #...# ..#.. #...# ....# . ..
....# ####. ##### ..#.. .###. ##### # .#
..... ..... ..... ..... ..... ..... . #.
..... ..... ..... ..... ..... ..... . ..

:;!?'"-_
. .. # ##### # #.# ... .....
. .. # ....# # #.# ... .....
. .. # ....# . ... ... .....
# .# # ..### . ... ### .....
. .. # ..#.. . ... ... .....
. .. . ..... . ... ... .....
# .# # ..#.. . ... ... .....
. #. . ..... . ... ... #####
. .. . ..... . ... ... .....

+=*/\|()
... ... ... ..# #.. # .# #.
... ... #.# ..# #.. # #. .#
.#. ### .#. .#. .#. # #. .#
### ... #.# .#. .#. # #. .#
.#. ### ... .#. .#. # #. .#
... ... ... #.. ..# # #. .#
... ... ... #.. ..# # .# #.
... ... ... ... ... # .. ..
... ... ... ... ... # .. ..

[]{}<>@#
## ## .## ##. ... ... ##### .#.#.
#. .# .#. .#. ..# #.. #...# .#.#.
#. .# .#. .#. .#. .#. #.### #####
#. .# #.. ..# #.. ..# #.#.# .#.#.
#. .# .#. .#. .#. .#. #.### #####
#. .# .#. .#. ..# #.. #.... .#.#.
## ## .## ##. ... ... ##### .#.#.
.. .. ... ... ... ... ..... .....
.. .. ... ... ... ... ..... .....

$%&^~
..#.. ##..# .##.. .#. ....
##### ##.#. #..#. #.# ....
#.#.. ...#. #..#. ... ##.#
##### ..#.. .##.. ... #.##
..#.# .#... #.#.# ... ....
##### .#.## #..#. ... ....
..#.. #..## .##.# ... ....
..... ..... ..... ... ....
..... ..... ..... ... ....
` + "\n`" + `
#.
.#
..
..
..
..
..
..
..
`

// glyphs maps printable ASCII to the face; the space is two cells of
// nothing.
var glyphs = func() map[byte]glyph {
	out := map[byte]glyph{' ': {w: 2}}
	for _, bank := range strings.Split(strings.TrimSpace(glyphTable), "\n\n") {
		lines := strings.Split(bank, "\n")
		chars := lines[0]
		if len(lines) != glyphRows+1 {
			panic("lettering bank " + chars + " needs a row of cells per glyph row")
		}
		for y, line := range lines[1:] {
			cells := strings.Split(line, " ")
			if len(cells) != len(chars) {
				panic("lettering bank " + chars + " has a row of another count: " + line)
			}
			for i, cell := range cells {
				g := out[chars[i]]
				if y > 0 && len(cell) != g.w || len(cell) > 8 {
					panic("lettering glyph " + chars[i:i+1] + " changes width")
				}
				g.w = len(cell)
				for x := range len(cell) {
					switch cell[x] {
					case '#':
						g.rows[y] |= 1 << x
					case '.':
					default:
						panic("lettering glyph " + chars[i:i+1] + " has a cell that is not # or .")
					}
				}
				out[chars[i]] = g
			}
		}
	}
	return out
}()

// glyphOf is c's glyph; one the face lacks draws as a question mark.
func glyphOf(c byte) glyph {
	if g, ok := glyphs[c]; ok {
		return g
	}
	return glyphs['?']
}

// lettering is text laid out in lines of at most cols cells: its particles
// on an 8-unit grid and its box in those units.
type lettering struct {
	text      string
	cols      int
	particles [][2]float32
	w, h      float32
}

// cellsOf is the width of text in cells, a cell between glyphs.
func cellsOf(text string) int {
	n := 0
	for i := range len(text) {
		n += glyphOf(text[i]).w + 1
	}
	return max(n-1, 0)
}

// wrapText breaks text into lines of at most cols cells: between words
// where it can, inside a word longer than a line, and never fewer than a
// glyph a line. cols <= 0 is one line.
func wrapText(text string, cols int) []string {
	if cols <= 0 || cellsOf(text) <= cols {
		return []string{text}
	}
	var lines []string
	line := ""
	for _, word := range strings.Fields(text) {
		if line != "" && cellsOf(line+" "+word) <= cols {
			line += " " + word
			continue
		}
		if line != "" {
			lines = append(lines, line)
			line = ""
		}
		for word != "" {
			n := 1
			for n < len(word) && cellsOf(word[:n+1]) <= cols {
				n++
			}
			if n == len(word) {
				line, word = word, ""
				break
			}
			lines, word = append(lines, word[:n]), word[n:]
		}
	}
	if line != "" {
		lines = append(lines, line)
	}
	return lines
}

// layoutLettering lays text out in lines of at most cols cells, each
// centered across the widest.
func layoutLettering(text string, cols int) lettering {
	lines := wrapText(text, cols)
	widest := 0
	for _, line := range lines {
		widest = max(widest, cellsOf(line))
	}
	l := lettering{text: text, cols: cols,
		w: float32(16 * widest),
		h: float32(8 * (len(lines)*letterLine + (len(lines)-1)*letterGap))}
	for n, line := range lines {
		x := widest - cellsOf(line) // in half cells: centered to a particle
		top := 1 + n*(letterLine+letterGap)
		for i := range len(line) {
			g := glyphOf(line[i])
			for y, row := range g.rows {
				for cx := range g.w {
					if row&(1<<cx) == 0 {
						continue
					}
					for _, d := range [][2]int{{0, 0}, {1, 0}, {0, 1}, {1, 1}} {
						px, py := x+2*cx+d[0], top+2*y+d[1]
						l.particles = append(l.particles, [2]float32{float32(8*px + 1), float32(8*py + 1)})
					}
				}
			}
			x += 2 * (g.w + 1)
		}
	}
	return l
}
