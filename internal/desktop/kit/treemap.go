package kit

import (
	"cmp"
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// MyGo's ui/chart draws axes, lines, bars and pies but no treemap, and
// CrychicUI's chart package is not used: the treemap is drawn here, each
// tile a positioned element so it keeps its tooltip, hover and accessible
// label, and moves to its new place when the values change.

// TreemapItem is one tile: its value sizes it, Detail is the value as its
// owner writes it (a size, a count) under the label.
type TreemapItem struct {
	ID, Label, Detail string
	Value             float64
}

// TreemapGroup is a category of tiles sharing a color, named with its total
// in the legend.
type TreemapGroup struct {
	ID, Label, Detail string
	Items             []TreemapItem
}

// TreemapOptions size the map; a zero Height is 280 DIP.
type TreemapOptions struct {
	Height float32
}

const (
	treemapGap      = 6
	treemapLabelW   = 56
	treemapLabelH   = 40
	treemapDetailH  = 56
	treemapDuration = 320 * time.Millisecond
)

// Treemap draws groups as a squarified treemap over a legend: the groups
// share the area by their totals, each group's items share its part by
// value, and every tile keeps close to square. A tile too small for its
// label shows it in its tooltip.
func Treemap(c *ui.Context, key any, groups []TreemapGroup, o TreemapOptions) ui.Element {
	p := theme.Of(c)
	height := o.Height
	if height <= 0 {
		height = 280
	}
	root := ui.Column(c.Key(key)).FillWidth().Gap(14)
	root.Children(func() {
		plot := ui.Box(c.Key("plot")).FillWidth().Height(height)
		bounds := plot.Bounds()
		if bounds.W <= 0 {
			// The first frame has no width to divide yet.
			c.Invalidate()
		}
		area := ui.Rect{W: bounds.W, H: height}
		totals := make([]float64, len(groups))
		for i, group := range groups {
			for _, item := range group.Items {
				totals[i] += max(0, item.Value)
			}
		}
		plot.Children(func() {
			for i, frame := range squarify(totals, inflate(area, treemapGap/2)) {
				if totals[i] <= 0 {
					continue
				}
				color := p.Series(i)
				values := make([]float64, len(groups[i].Items))
				for j, item := range groups[i].Items {
					values[j] = max(0, item.Value)
				}
				for j, tile := range squarify(values, frame) {
					if values[j] <= 0 {
						continue
					}
					treemapTile(c, groups[i], groups[i].Items[j], inflate(tile, -treemapGap/2), color)
				}
			}
		})
		ui.Row(c.Key("legend")).FillWidth().Wrap().Gap(18).Children(func() {
			for i, group := range groups {
				if totals[i] <= 0 {
					continue
				}
				ui.Row(c.Key("legend-" + group.ID)).Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Box(c).Size(10, 10).Radius(3).Background(p.Series(i))
					ui.Text(c, group.Label).FontSize(13)
					if group.Detail != "" {
						ui.Text(c, group.Detail).FontSize(13).TextColor(p.MutedForeground)
					}
				})
			}
		})
	})
	return root
}

func treemapTile(c *ui.Context, group TreemapGroup, item TreemapItem, r ui.Rect, color ui.Color) {
	p := theme.Of(c)
	if r.W <= 0 || r.H <= 0 {
		return
	}
	// MyGo keeps a box at least as large as its padding and border. Keep
	// label padding inside the tile so a small tile preserves its gap.
	tile := ui.Column(c.Key("tile-" + group.ID + "/" + item.ID)).Absolute().Clip().Radius(p.Radius)
	x := tile.Animate("x", r.X, treemapDuration)
	y := tile.Animate("y", r.Y, treemapDuration)
	w := tile.Animate("w", r.W, treemapDuration)
	h := tile.Animate("h", r.H, treemapDuration)
	fill := float32(0.14)
	if tile.Hovered() {
		fill = 0.24
	}
	label := item.Label
	if item.Detail != "" {
		label += " · " + item.Detail
	}
	if group.Label != "" {
		label += " · " + group.Label
	}
	tile.Left(x).Top(y).Width(w).Height(h).Border(min(1, w/2, h/2), color.Alpha(0.38)).
		Background(color.Alpha(fill)).Label(label).Tooltip(label)
	tile.Children(func() {
		if r.W < treemapLabelW || r.H < treemapLabelH {
			return
		}
		ui.Column(c).Fill().Padding(10, 12).Gap(2).Children(func() {
			ui.Text(c, item.Label).FontSize(13).FontWeight(500).TextColor(color).SingleLine()
			if r.H >= treemapDetailH {
				detail := item.Detail
				if group.Label != "" {
					detail = joinDetail(detail, group.Label)
				}
				ui.Text(c, detail).FontSize(12).TextColor(color.Alpha(0.72)).SingleLine()
			}
		})
	})
}

func joinDetail(value, group string) string {
	if value == "" {
		return group
	}
	return value + " · " + group
}

func inflate(r ui.Rect, d float32) ui.Rect {
	return ui.Rect{X: r.X - d, Y: r.Y - d, W: max(0, r.W+2*d), H: max(0, r.H+2*d)}
}

// squarify divides r among values in proportion, as Bruls, Huizing and van
// Wijk's squarified treemap does: the largest first, each row of tiles laid
// along the shorter side while adding a tile keeps the row's worst aspect
// ratio from growing. The rectangles come back in the values' order; a
// value of zero or less gets an empty one.
func squarify(values []float64, r ui.Rect) []ui.Rect {
	out := make([]ui.Rect, len(values))
	order := make([]int, 0, len(values))
	total := 0.0
	for i, v := range values {
		if v > 0 {
			order = append(order, i)
			total += v
		}
	}
	if total <= 0 || r.W <= 0 || r.H <= 0 {
		return out
	}
	slices.SortStableFunc(order, func(a, b int) int { return cmp.Compare(values[b], values[a]) })
	scale := float64(r.W) * float64(r.H) / total
	area := func(i int) float64 { return values[i] * scale }
	worst := func(row []int, side float64) float64 {
		sum, lo, hi := 0.0, area(row[0]), area(row[0])
		for _, i := range row {
			a := area(i)
			sum += a
			lo, hi = min(lo, a), max(hi, a)
		}
		s2, sum2 := side*side, sum*sum
		return max(s2*hi/sum2, sum2/(s2*lo))
	}
	free := r
	place := func(row []int) {
		sum := 0.0
		for _, i := range row {
			sum += area(i)
		}
		if free.W >= free.H {
			// A column at the left, as wide as the row's area needs.
			width := float32(sum / float64(free.H))
			y := free.Y
			for _, i := range row {
				h := float32(area(i) / float64(width))
				out[i] = ui.Rect{X: free.X, Y: y, W: width, H: h}
				y += h
			}
			free.X, free.W = free.X+width, max(0, free.W-width)
		} else {
			height := float32(sum / float64(free.W))
			x := free.X
			for _, i := range row {
				w := float32(area(i) / float64(height))
				out[i] = ui.Rect{X: x, Y: free.Y, W: w, H: height}
				x += w
			}
			free.Y, free.H = free.Y+height, max(0, free.H-height)
		}
	}
	var row []int
	for _, i := range order {
		side := float64(min(free.W, free.H))
		if len(row) == 0 || worst(append(row, i), side) <= worst(row, side) {
			row = append(row, i)
			continue
		}
		place(row)
		row = []int{i}
	}
	if len(row) > 0 {
		place(row)
	}
	return out
}
