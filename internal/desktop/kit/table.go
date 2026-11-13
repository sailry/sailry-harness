package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The application's table look around MyGo's ui.Table, which has no frame,
// header fill or row separator of its own and pads each cell itself: a
// 1 DIP Border frame at the base radius, the header filled Muted, a Border
// line between rows, and rows of one exact height with their content
// centered. The table inside is built under theme.Neutral, whose spacing
// gives the header's height and the cell's padding (tableMetrics); the hover
// and chosen fills, sorting, selection and scrolling stay MyGo's.

// tableMetrics are the header's height and a cell's padding on each side as
// MyGo's table lays them out under the neutral theme: 32 and 6 DIP.
func tableMetrics(c *ui.Context) (head, pad float32) {
	neutral := theme.Neutral(c)
	return neutral.Space(8), neutral.Space(1.5)
}

// TableFrame makes frame the border around a table that is its one child:
// the frame, clipped to its radius, with the header's fill drawn under the
// table's head.
func TableFrame(c *ui.Context, frame ui.Element) ui.Element {
	p := theme.Of(c)
	head, _ := tableMetrics(c)
	return frame.Border(1, p.Border).Radius(p.Radius).Clip().Draw(func(painter *ui.Painter, bounds ui.Rect) {
		painter.FillRadii(ui.Rect{X: bounds.X, Y: bounds.Y, W: bounds.W, H: head + 1}, [4]float32{p.Radius, p.Radius, 0, 0}, p.Muted)
	})
}

// TableHeight is the height of a framed table that shows rows rows of
// rowHeight without scrolling: the header, its divider and the frame.
func TableHeight(c *ui.Context, rows int, rowHeight float32) float32 {
	head, _ := tableMetrics(c)
	return head + 3 + float32(rows)*rowHeight
}

// TableCell is a cell of a framed table: a column that makes its row
// rowHeight high with the padding MyGo gives the cell, its content centered
// in it. It draws the line under its row across that padding unless last,
// which the table's last row is. An optional background fills the whole cell,
// including MyGo's padding. It reads the laid-out bounds so a data table's
// alternating fill can yield to the hovered or chosen row.
func TableCell(c *ui.Context, rowHeight float32, last bool, background ...func(ui.Rect) ui.Color) ui.Element {
	p := theme.Of(c)
	_, pad := tableMetrics(c)
	padX := theme.Neutral(c).Space(2.5)
	cell := ui.Column(c).MinHeight(rowHeight - 2*pad).AlignSelf(ui.Stretch).Justify(ui.Center)
	if !last || len(background) != 0 {
		cell.Draw(func(painter *ui.Painter, bounds ui.Rect) {
			if len(background) != 0 {
				if fill := background[0](bounds); fill != ui.Transparent {
					painter.Fill(ui.Rect{X: bounds.X - padX, Y: bounds.Y - pad, W: bounds.W + 2*padX, H: bounds.H + 2*pad}, fill, 0)
				}
			}
			// The cell's own box clips the line to the column.
			if !last {
				painter.Fill(ui.Rect{X: bounds.X - 4096, Y: bounds.Y + bounds.H + pad - 1, W: 8192, H: 1}, p.Border, 0)
			}
		})
	}
	return cell
}
