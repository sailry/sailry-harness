// Dialog's body is adapted from Lobe MyGo's feedback.Modal and present
// (github.com/lobehub/lobe-mygo pkg/feedback/modal.go and dialog.go at
// 5b3ecde):
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

package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// Field is a dialog's labeled control: a 14 DIP label over it, a muted
// hint under it ("" for none).
func Field(c *ui.Context, key any, label, hint string, control func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Gap(6).Children(func() {
		ui.Text(c, label).FontSize(14).FontWeight(500).TextColor(p.PopoverForeground)
		control()
		if hint != "" {
			ui.Text(c, hint).FontSize(12).TextColor(p.MutedForeground)
		}
	})
}

// Dialog is Lobe MyGo's Modal placed as Kit places its dialogs: a tenth
// of the window down rather than centered (dialog.rs margin_top), on
// DialogBase, the backdrop dimming in and the panel (Floating) scaling and
// fading in, and out again as it closes while the caller still builds it,
// with a close button at its top right; Escape and a click on the backdrop
// close it. fn builds the content. It returns the panel, or nil while
// closed.
//
// Changes: our placement, padding and gap, the panel through Floating,
// the backdrop in the palette's Backdrop, no title or footer of its own
// (the callers build theirs), the eases inlined.
func Dialog(c *ui.Context, open *bool, fn func()) ui.Element {
	return dialog(c, open, true, fn)
}

// BareDialog is Dialog without its close button, for content that draws
// its own (the welcome guide's banner).
func BareDialog(c *ui.Context, open *bool, fn func()) ui.Element {
	return dialog(c, open, false, fn)
}

func dialog(c *ui.Context, open *bool, closer bool, fn func()) ui.Element {
	p := theme.Of(c)
	w, h := c.Size()
	st := ui.Local(ui.Box(c).Absolute().Size(0, 0), "dialog", func() dialogState { return dialogState{} })
	if *open {
		st.shown, st.closing = true, false
	} else if st.shown {
		st.closing = true
	}
	if !st.shown {
		return ui.Element{}
	}
	gone := false
	var out ui.Element
	ui.DialogBase(c, &st.shown, func(back, panel ui.Element) {
		out = panel
		if !st.entered {
			st.entered = true
			panel.Jump("shown", 0)
			back.Jump("dim", 0)
		}
		target, anim := float32(1), dialogIn
		if st.closing {
			target, anim = 0, dialogOut
			back.IgnoreInput(true)
		}
		shown := panel.AnimateTo("shown", target, anim)
		dim := back.AnimateTo("dim", target, dialogFade)
		gone = st.closing && shown <= 0 && dim <= 0
		from := float32(0.97)
		if st.closing {
			from = 0.98
		}
		scale := from + (1-from)*shown
		back.Justify(ui.Start).Padding(h/10, 0, 0, 0).Background(p.Backdrop().Alpha(0.4 * dim))
		Floating(c, panel.Padding(20).Gap(12).MaxWidth(w-40).MaxHeight(h*0.9-20)).Scale(scale, scale).Opacity(shown)
		fn()
		if !closer {
			return
		}
		b := ui.ButtonBase(c.Key("dialog-close")).Label(i18n.T(c, "close")).Size(28, 28).Radius(p.Radius).Absolute().Top(12).Right(12).
			Center().Cursor(ui.CursorPointer).Transition(toastHover)
		if b.Hovered() {
			b.Background(p.Hover).TextColor(p.Foreground)
		} else {
			b.TextColor(p.MutedForeground)
		}
		b.Children(func() { ui.Icon(c, icons.Close).FontSize(16) })
		if b.Clicked() {
			*open = false
		}
	})
	switch {
	case !st.shown:
		*open, st.shown, st.closing = false, true, true
		c.Invalidate()
	case gone:
		*st = dialogState{}
		c.Invalidate()
	}
	return out
}

// dialogState keeps a dialog built while it leaves (Lobe MyGo's present):
// DialogBase stops building it the frame it closes, so its exit would not
// play.
type dialogState struct{ shown, closing, entered bool }

var (
	dialogIn   = ui.Animation{Duration: 220 * time.Millisecond, Ease: softEase}
	dialogOut  = ui.Animation{Duration: 120 * time.Millisecond, Ease: ui.CubicBezier(0.4, 0, 1, 1)}
	dialogFade = ui.Animation{Duration: 180 * time.Millisecond, Ease: softEase}
)
