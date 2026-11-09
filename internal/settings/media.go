package settings

import (
	"hash/fnv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/theme"
)

// TileSize is the leading tile slot of a media row (ui/media_row.rs TILE).
const TileSize = 36

// MediaRow is the settings lists' shared record (ui/media_row.rs): a 36 DIP
// leading tile, then a title of the body size in medium weight with an
// optional badge, and a one-line description in muted text at 70%, 2 DIPs
// below it. Either function may be nil.
func MediaRow(c *ui.Context, title, description string, tile, badge func()) ui.Element {
	p := theme.Of(c)
	return ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(12).Children(func() {
		if tile != nil {
			ui.Row(c).Size(TileSize, TileSize).Shrink(0).Center().Children(tile)
		}
		ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
			ui.Row(c).MinWidth(0).Gap(8).Children(func() {
				ui.Text(c, title).FontSize(16).FontWeight(500).LineHeight(1.3).SingleLine().Shrink(1).TextColor(p.Foreground)
				if badge != nil {
					ui.Row(c).Shrink(0).Children(badge)
				}
			})
			if description != "" {
				ui.Text(c, description).FontSize(14).LineHeight(1.3).SingleLine().TextColor(p.Description())
			}
		})
	})
}

// Tile gradients of plugins/emblem.rs, from top to bottom.
var tints = [][2]string{
	{"#3da9fc", "#2580f4"}, // blue
	{"#9b7cf8", "#7250e3"}, // violet
	{"#35c5a9", "#199b87"}, // teal
	{"#f9b74e", "#e68926"}, // amber
	{"#9398a3", "#656a75"}, // gray
}

// Tile is the emblem of a record: a gradient chosen by the name's FNV hash,
// radius 11, with a white glyph 5/9 of its size, or the name's initial.
func Tile(c *ui.Context, name string, glyph *ui.SVG) {
	h := fnv.New32a()
	h.Write([]byte(name))
	tint := tints[h.Sum32()%uint32(len(tints))]
	ui.Row(c).Size(TileSize, TileSize).Radius(11).Center().
		Gradient(ui.Hex(tint[0]), ui.Hex(tint[1]), 180).TextColor(ui.Hex("#ffffff")).Children(func() {
		if glyph != nil {
			ui.Icon(c, glyph).FontSize(TileSize * 5 / 9)
			return
		}
		r := []rune(name)
		ui.Text(c, string(r[:1])).FontSize(TileSize * 5 / 9).FontWeight(600)
	})
}
