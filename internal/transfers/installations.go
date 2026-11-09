package transfers

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Visible reports whether the rail shows the button: while there are
// requests (installations::visible).
func Visible(q *Queue) bool { return q != nil && len(q.Entries) > 0 }

// Button is the rail's installations button (installations/popover.rs): a
// 36 DIP ghost button with the download arrow, smaller inside a progress
// ring while requests run, with a danger dot while one ended without
// success; it opens the requests in a popover 320 wide to its right.
// retry runs again a retried request's work (preview_notice in the
// preview).
func Button(c *ui.Context, q *Queue, open *bool, retry func(e Entry)) {
	p := theme.Of(c)
	label := i18n.T(c, "installation_title")
	active, attention := false, false
	for _, e := range q.Entries {
		active = active || e.Active()
		attention = attention || e.Terminal()
	}
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("installations")).Label(label), *open).Size(36, 36).Tooltip(label).
		OnClick(func() { *open = !*open })
	b.HasPopup(ui.PopupDialog).Expanded(*open)
	b.Children(func() {
		ui.Row(c.Key("mark")).Size(20, 20).Center().Children(func() {
			size := float32(16)
			if active {
				size = 12
				value, measured := Overall(q.Entries)
				ring(c, p.Progress, value/100, !measured)
			}
			ui.Icon(c, icons.Download).FontSize(size)
		})
	})
	if attention {
		b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			pt.Fill(ui.Rect{X: r.X + r.W - 12, Y: r.Y + 6, W: 6, H: 6}, p.Danger, 3)
		})
	}
	w, h := c.Size()
	panel := ui.Popover(c, b, open, func() {
		ui.Row(c.Key("heading")).Height(40).PaddingX(8).Shrink(0).Children(func() {
			ui.Text(c, label).FontSize(14).FontWeight(600)
		})
		if len(q.Entries) == 0 {
			kit.Empty(c, icons.Download, i18n.T(c, "installation_empty"), "", nil)
			return
		}
		ui.Scroll(c.Key("list")).MaxHeight(min(h*0.55, 400)).Children(func() {
			for _, e := range q.Entries {
				row(c, e, func() { q.Remove(e.Key) }, func() {
					q.Retry(e.Key)
					retry(e)
				})
			}
		})
	})
	if panel != (ui.Element{}) {
		panel.AttachTo(b, ui.AnchorRight, ui.AnchorLeft).MarginStart(8).Width(min(320, w-32)).Padding(8).Background(p.Floating())
	}
}

// row is a request (rows.rs render_item): 56 high, a 28 DIP tile with the
// kind's icon (danger or warning tinted when it failed or is uncertain),
// the title over the status and where it runs, a 2 DIP bar while it runs,
// the measure, a clock while queued, then Remove on hover and Retry. The
// pointer resting on it shows the whole detail.
func row(c *ui.Context, e Entry, remove, retry func()) {
	p := theme.Of(c)
	phase := i18n.T(c, e.Label())
	if !e.Connected && !e.Terminal() {
		phase = i18n.T(c, "installation_offline", "phase", phase)
	}
	where := i18n.T(c, "installation_node")
	if e.Controller {
		where = i18n.T(c, "installation_controller")
	}
	summary := phase + " · " + where
	status := summary
	if m := e.Measure(); m != "" {
		status += " · " + m
		if e.Speed > 0 {
			status += " · " + i18n.T(c, "installation_speed", "value", formatMiB(e.Speed))
		}
	}
	detail := e.Title + "\n" + status
	if e.Error != "" {
		detail += "\n" + e.Error
	}
	icon, tone, tile := map[Kind]*ui.SVG{Plugin: icons.Package, Model: icons.Cpu, Language: icons.Code}[e.Kind], p.Foreground, p.Muted
	switch e.Status {
	case Failed:
		icon, tone, tile = icons.CircleAlert, p.Danger, p.Danger.Alpha(0.1)
	case Uncertain:
		icon, tone, tile = icons.CircleQuestion, p.Warning, p.Warning.Alpha(0.1)
	}
	r := ui.Row(c.Key(e.Key)).Height(56).PaddingX(8).Gap(8).Radius(p.Radius).Label(detail).Tooltip(detail)
	hovered := r.Hovered()
	if hovered {
		r.Background(p.ListHover)
	}
	r.Children(func() {
		ui.Row(c.Key("tile")).Size(28, 28).Shrink(0).Radius(p.Radius).Background(tile).Center().Children(func() {
			ui.Icon(c, icon).FontSize(16).TextColor(tone)
		})
		ui.Column(c.Key("text")).Grow(1).Shrink(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, e.Title).FontSize(14).SingleLine()
			ui.Text(c, summary).FontSize(14).SingleLine().TextColor(p.MutedForeground)
			if e.Active() {
				v, measured := e.Measured()
				if !measured {
					v = -100
				}
				ui.Progress(c.Key("progress"), v/100).FillWidth().Height(2).Label(status)
			}
		})
		if m := e.Measure(); m != "" {
			ui.Text(c, m).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
		}
		if e.Status == Waiting || e.Status == Queued {
			ui.Row(c.Key("queued")).PaddingX(4).Shrink(0).Children(func() {
				ui.Icon(c, icons.Clock).FontSize(16).TextColor(p.MutedForeground)
			})
		}
		if !e.Actionable {
			return
		}
		ui.Row(c.Key("actions")).Gap(4).Shrink(0).Children(func() {
			x := kit.IconButton(c.Key("remove"), icons.Close, i18n.T(c, "installation_remove"), 24, false).OnClick(remove)
			if !hovered && !x.FocusVisible() {
				x.Opacity(0)
			}
			if e.Retryable {
				kit.IconButton(c.Key("retry"), icons.Redo, i18n.T(c, "installation_retry"), 24, false).TextColor(tone).OnClick(retry)
			}
		})
	})
}

// formatMiB writes bytes a second in MiB with one decimal.
func formatMiB(v float64) string { return strconv.FormatFloat(v/1048576, 'f', 1, 64) }

// ring is the progress ring around the installing arrow
// (ui/status_ring.rs progress): a 1.5 DIP track and arc from the top,
// turning while unmeasured, at most 60 frames a second. Custom drawing:
// MyGo's Progress is a bar and its Spinner has spokes.
func ring(c *ui.Context, color ui.Color, value float64, spinning bool) {
	ui.Box(c.Key("ring")).Absolute().Left(0).Top(0).Right(0).Bottom(0).PassThrough().Draw(func(pt *ui.Painter, r ui.Rect) {
		const width = 1.5
		rad := min(r.W, r.H)/2 - width/2
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		pt.StrokePath(circle(cx, cy, rad), width, color.Alpha(0.25))
		from, to := float32(0), float32(min(max(value, 0), 1))
		if spinning {
			pt.AnimationFrameRate(kit.FPS)
			from = float32(pt.Now().UnixMilli()%1000) / 1000 * 0.75
			to = from + 0.25
		}
		if to > from {
			pt.StrokePath(circle(cx, cy, rad).Trim(from, to), width, color)
		}
	})
}

// circle is a circle from its top, clockwise.
func circle(cx, cy, r float32) *ui.Path {
	const k = 0.5522847498
	path := &ui.Path{}
	path.MoveTo(cx, cy-r)
	path.CubeTo(cx+r*k, cy-r, cx+r, cy-r*k, cx+r, cy)
	path.CubeTo(cx+r, cy+r*k, cx+r*k, cy+r, cx, cy+r)
	path.CubeTo(cx-r*k, cy+r, cx-r, cy+r*k, cx-r, cy)
	path.CubeTo(cx-r, cy-r*k, cx-r*k, cy-r, cx, cy-r)
	return path
}
