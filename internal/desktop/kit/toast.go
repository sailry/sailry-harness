// ToastHost is adapted from Lobe MyGo's feedback.ToastHost
// (github.com/lobehub/lobe-mygo pkg/feedback/toast.go at 5b3ecde):
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
// Changes: top-center placement, Sailry's palette and icons (the panel through
// Floating), the eases inlined, body actions and no close button. ShowToast is
// left out: the app adds MyGo toasts (c.AddToast, Error, Notify, Retry) as before.

package kit

import (
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

const (
	toastWidth = 360
	toastGap   = 12
	toastPeek  = 12
)

// ToastBodyAction in ui.Toast.Data makes the toast's body run its Action
// instead of showing an action button.
type ToastBodyAction struct{}

// Lobe MyGo's motion eases.
var (
	toastEase = ui.CubicBezier(0.22, 1, 0.36, 1)
	cssEase   = ui.CubicBezier(0.25, 0.1, 0.25, 1)
	softEase  = ui.CubicBezier(0.32, 0.72, 0, 1)

	toastStack = ui.Animation{Duration: 400 * time.Millisecond, Ease: toastEase}
	toastFade  = ui.Animation{Duration: 200 * time.Millisecond, Ease: EaseOut}
	toastSize  = ui.Animation{Duration: 150 * time.Millisecond, Ease: cssEase}
	toastEnter = ui.ElementTransition{Duration: 400 * time.Millisecond, Ease: toastEase, Colors: true,
		Enter: &ui.Motion{YPercent: -150}, Exit: &ui.Motion{YPercent: -150}}
	toastHover = ui.ElementTransition{Duration: 160 * time.Millisecond, Ease: softEase, Colors: true}
)

// ToastHost draws the window's toasts in place of MyGo's look, at the
// top center (under the header): the newest in front, sliding down from
// the top, with the older ones peeking below it, spread out while the pointer rests on them. A toast has its
// type's icon and color (info, success, warning, error), the title over
// the description and its action. Publishers use ToastDuration so every toast
// expires through MyGo's timer, which pauses while hovered or focused. Call it
// once, last in the window's view, so the toasts lie over an open dialog.
func ToastHost(c *ui.Context) ui.Element {
	return ui.ToastViewportBase(c, func(viewport ui.Element, toasts []ui.Toast) {
		viewport.Padding(56, 16, 16, 16).AlignItems(ui.Center).Justify(ui.Start)
		stack := ui.Box(c).Width(toastWidth).MaxWidthPercent(100)
		heights := *ui.Local(stack, "heights", func() map[string]float32 { return map[string]float32{} })
		expanded := stack.Hovered()
		n := len(toasts)
		if len(heights) > n {
			// The heights of the toasts that left.
			for id := range heights {
				if !slices.ContainsFunc(toasts, func(t ui.Toast) bool { return t.ID == id }) {
					delete(heights, id)
				}
			}
		}
		front := float32(0)
		if n > 0 {
			front = heights[toasts[n-1].ID]
		}
		total := front + toastPeek*float32(max(n-1, 0))
		if expanded {
			total = 0
			for _, t := range toasts {
				total += heights[t.ID]
			}
			total += toastGap * float32(max(n-1, 0))
		}
		stack.Height(total)
		layouts := make([]toastLayout, n)
		offset := float32(0)
		for k := n - 1; k >= 0; k-- {
			depth := n - 1 - k
			scale := 1 - 0.05*float32(depth)
			l := toastLayout{y: float32(depth)*toastPeek + (1-scale)*front, scale: scale, collapsed: !expanded && depth > 0, front: front}
			if expanded {
				l.y, l.scale = offset, 1
			}
			offset += heights[toasts[k].ID] + toastGap
			layouts[k] = l
		}
		stack.Children(func() {
			for k, t := range toasts {
				toast(c, t, heights, layouts[k])
			}
		})
	})
}

type toastLayout struct {
	y, scale, front float32
	collapsed       bool
}

// toast is a toast in the stack, which moves it with transforms of its
// own; its Transition plays only its Enter and Exit.
func toast(c *ui.Context, t ui.Toast, heights map[string]float32, l toastLayout) {
	p := theme.Of(c)
	parts := ui.ToastBase(c, t)
	root := Floating(c, parts.Root.Absolute().Left(0).Right(0).Top(0).TransformOrigin(0.5, 0).Clip()).FontSize(14)
	root.Transition(toastEnter)
	height := heights[t.ID]
	if l.collapsed && l.front > 0 {
		height = l.front
	}
	if height > 0 {
		root.Height(root.AnimateTo("height", height, toastSize))
	}
	root.Translate(0, root.AnimateTo("y", l.y, toastStack))
	scale := root.AnimateTo("scale", l.scale, toastStack)
	root.Scale(scale, scale)
	shown := float32(1)
	if l.collapsed {
		shown = 0
	}
	opacity := root.AnimateTo("content", shown, toastFade)
	_, bodyAction := t.Data.(ToastBodyAction)
	var body ui.Element
	root.Children(func() {
		if bodyAction {
			body = parts.ActionButton().Justify(ui.Start).Cursor(ui.CursorPointer)
		} else {
			body = ui.Row(c)
		}
		body.Gap(12).Padding(12, 16).AlignItems(ui.Start).Shrink(0).Opacity(opacity)
		body.Children(func() {
			icon, color := toastIcon(p, t.Type)
			ui.Row(c).Height(21).AlignItems(ui.Center).Shrink(0).Children(func() {
				ui.Icon(c, icon).FontSize(18).TextColor(color)
			})
			ui.Column(c).Grow(1).MinWidth(0).Children(func() {
				ui.Row(c).Gap(8).AlignItems(ui.Start).Children(func() {
					if t.Title != "" {
						ui.Text(c, t.Title).FontWeight(500).LineHeight(1.5).Grow(1)
					} else {
						ui.Text(c, t.Description).LineHeight(1.5).Grow(1)
					}
				})
				if t.Title != "" && t.Description != "" {
					ui.Text(c, t.Description).FontSize(13).LineHeight(1.5).TextColor(p.MutedForeground).Margin(4, 0, 0, 0)
				}
				if t.Action != "" && !bodyAction {
					ui.Row(c).Justify(ui.End).Margin(8, 0, 0, 0).Children(func() {
						toastAction(c, p, parts.ActionButton(), t.Action)
					})
				}
			})
		})
	})
	if h := body.Bounds().H; h > 0 && h != heights[t.ID] {
		heights[t.ID] = h
		c.Invalidate()
	}
}

// toastIcon is a toast type's icon and color.
func toastIcon(p *theme.Palette, typ string) (*ui.SVG, ui.Color) {
	switch typ {
	case "info":
		return icons.Info, p.Primary
	case "success":
		return icons.CircleCheck, p.Success
	case "warning":
		return icons.TriangleAlert, p.Warning
	case "error":
		return icons.CircleX, p.Danger
	}
	return icons.Info, p.Foreground
}

func toastAction(c *ui.Context, p *theme.Palette, b ui.Element, label string) {
	bg := p.ButtonPrimary
	switch {
	case b.Pressed():
		bg = p.ButtonPrimaryActive
	case b.Hovered():
		bg = p.ButtonPrimaryHi
	}
	b.Height(28).PaddingX(12).Radius(p.Radius / 2).Background(bg).TextColor(p.OnPrimary).
		FontSize(12).FontWeight(500).Cursor(ui.CursorPointer).Transition(toastHover)
	b.Children(func() { ui.Text(c, label).SingleLine() })
}
