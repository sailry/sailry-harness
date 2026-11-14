// InputOTP is adapted from Lobe MyGo's field.InputOTP
// (github.com/lobehub/lobe-mygo pkg/field/otp.go at 5b3ecde, with the
// bits of fieldRow and styleField from pkg/field/input.go it uses):
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
// Changes: the cells take a text input's look from MyGo's theme (the
// Surface, the input border, the radius, the focus color on the cell the
// next character goes in), one size and one variant.

package kit

import (
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// InputOTPProps are an InputOTP's options.
type InputOTPProps struct {
	// Length is how many characters the code has: 6 when zero.
	Length int
	// Alphanumeric takes letters as well as digits.
	Alphanumeric bool
	// OnComplete runs with the code once all of its characters are in.
	OnComplete func(code string)
	// Size is the cells' size: 32 square by default, 48 by 56 with a 24
	// DIP character for SizeLarge (a pairing code).
	Size Size
	// Fill stretches the row to its container's width, the cells sharing
	// it, so the code lines up with the fields above and below it.
	Fill bool
}

// InputOTP edits a one-time code in a row of cells, one character each,
// which typing fills from the first and Backspace empties from the last,
// over a hidden text input that takes the keys, the paste and the
// accessible name. It returns that input.
func InputOTP(c *ui.Context, key any, value *string, o InputOTPProps) ui.Element {
	p, t := theme.Of(c), c.Theme()
	n := o.Length
	if n <= 0 {
		n = 6
	}
	clean := make([]rune, 0, n)
	for _, r := range *value {
		if len(clean) < n && (unicode.IsDigit(r) || o.Alphanumeric && unicode.IsLetter(r)) {
			clean = append(clean, r)
		}
	}
	if string(clean) != *value {
		*value = string(clean)
	}
	w, h, font, radius := float32(32), float32(32), float32(14), p.Radius
	if o.Size == SizeLarge {
		w, h, font, radius = 48, 56, 24, p.RadiusLg
	}
	root := ui.Row(c.Key(key)).Gap(8).AlignItems(ui.Center).AlignSelf(ui.Start).Shrink(0)
	if o.Fill {
		root.AlignSelf(ui.Stretch)
	}
	last := ui.Local(root, "last", func() string { return *value })
	if *value != *last {
		*last = *value
		if len(clean) == n && o.OnComplete != nil {
			o.OnComplete(*value)
		}
	}
	var input ui.Element
	root.Children(func() {
		input = ui.TextInputBase(c.Key("input"), value).MaxLength(n).Absolute().Left(0).Top(0).Right(0).Bottom(0).
			Opacity(0).FocusRing(false)
		focused := input.Focused()
		for i := range n {
			active := focused && (i == len(clean) || i == n-1 && len(clean) == n)
			border := p.Input
			if active {
				border = t.Focus
			}
			cell := ui.Row(c.Key(i))
			if o.Fill {
				cell.Grow(1).Basis(0).MinWidth(w)
			} else {
				cell.Width(w)
			}
			cell.MinHeight(h).Radius(radius).Background(t.Surface).Border(1, border).
				AlignItems(ui.Center).Justify(ui.Center).FontSize(font).TextColor(p.Foreground).IgnoreInput(true).
				Children(func() {
					if i < len(clean) {
						ui.Text(c, string(clean[i])).FontFeatures("tnum")
					}
				})
		}
	})
	return input
}
