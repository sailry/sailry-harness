// Result is adapted from Lobe MyGo's feedback.Result
// (github.com/lobehub/lobe-mygo pkg/feedback/result.go at 5b3ecde):
//
// MIT License
//
// Copyright (c) 2026 LobeHub
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Changes: sized as Kit's empty state in the Rust app (empty_state.rs
// message over component/empty.rs): the icon at 24 DIP always in a 48 DIP
// round tile (EmptyMedia size_12, icon size_6), 12 between the parts
// (EmptyHeader gap_3), the title at 14 in normal weight and the muted
// color (EmptyTitle text_sm), the subtitle at 14 in the description color
// (EmptyDescription text_sm), both wrapping within 320 (max_w_80), 40
// above and below (list_content py_10), the actions 4 further down (the
// 16 of the terminal's panel gap_4); the outcome statuses Success and
// Warning dropped, as an operation's outcome is only a toast, leaving the
// empty state's Info and the load failure's Error; Card for a group
// without rows (empty_state.rs card); internal/icons and the palette.

package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// ResultStatus picks a Result's icon and its tint.
type ResultStatus int

const (
	// ResultInfo is an empty state: the muted tile.
	ResultInfo ResultStatus = iota
	// ResultError is a view that failed to load: the danger tint.
	ResultError
)

// ResultProps is what a Result shows.
type ResultProps struct {
	// Status picks the icon and its tint.
	Status ResultStatus
	// Title and SubTitle say what is there, or what failed to load;
	// SubTitle may be "".
	Title, SubTitle string
	// Icon replaces the status's icon in its tile.
	Icon *ui.SVG
	// Extra builds the actions below, as buttons, such as Retry; nil for
	// none.
	Extra func()
	// Card frames it in a dashed card, as a group without rows
	// (settings/group.rs Group::empty).
	Card bool
}

// resultWidth is the widest a Result's text runs (empty_state.rs
// content, max_w_80).
const resultWidth = 320

// Result is an empty state or a load failure, centered: its icon in a
// round tile, a title, a subtitle and actions. It is never an operation's
// outcome, which is only a toast.
func Result(c *ui.Context, p ResultProps) ui.Element {
	pal := theme.Of(c)
	glyph, fg, bg := icons.Info, pal.MutedForeground, pal.Muted
	if p.Status == ResultError {
		glyph, fg, bg = icons.Close, pal.Danger, pal.Danger.Alpha(0.12)
	}
	if p.Icon != nil {
		glyph = p.Icon
	}
	root := ui.Column(c.Key("result")).Padding(40, 16).Gap(12).AlignItems(ui.Center)
	if p.Card {
		root.Border(1, pal.Border).BorderStyle(ui.BorderDashed).Radius(pal.RadiusLg).Background(pal.Surface)
	}
	return root.Children(func() {
		ui.Row(c.Key("media")).Size(48, 48).Radius(24).Center().Background(bg).Shrink(0).Children(func() {
			ui.Icon(c, glyph).FontSize(24).TextColor(fg)
		})
		ui.Text(c, p.Title).FontSize(14).TextAlign(ui.Center).MaxWidth(resultWidth).TextColor(pal.MutedForeground)
		if p.SubTitle != "" {
			ui.Text(c, p.SubTitle).FontSize(14).TextAlign(ui.Center).MaxWidth(resultWidth).TextColor(pal.Description())
		}
		if p.Extra != nil {
			ui.Row(c).Padding(4, 0, 0, 0).Gap(8).Wrap().Justify(ui.Center).AlignItems(ui.Center).Children(p.Extra)
		}
	})
}
