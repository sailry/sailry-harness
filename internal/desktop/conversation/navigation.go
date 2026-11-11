package conversation

import (
	"math"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Layout metrics.
const (
	railWidth  = 50
	trackPad   = 12 // the rail's left padding, before its track
	railMin    = theme.ContentWidth + 2*railWidth
	tickSlot   = 10 // a tick's share of the track at most
	tickHit    = 4  // the hit margin around a tick
	tickInset  = 2
	tickHeight = 2.5
	tickReach  = 10  // how far from the hover ticks still widen visibly
	previewW   = 280 // the hovered tick's preview
)

// navigation is the transcript's message navigation: a rail along the left
// edge of the pane that holds the transcript and the composer, with a tick
// for each turn loaded, spread over the pane's height, centered, at most
// tickSlot DIPs apart. The turn at the top of the view, or
// the last while the list shows its end, ticks in the foreground, the others
// muted. Ticks near the pointer widen and brighten with a Gaussian falloff,
// and the one under it shows the turn's first line beside it. A press on a
// tick shows that turn at the top, dragging scrubs through them, and Up,
// Down, Home and End move a turn while the rail has the focus. It shows only
// where the transcript is wider than the content and a rail on each side, and
// not for a delegated session's read-only transcript.
//
// The pane's width, the transcript's, is known as it paints, so the box over
// it records it there and the rail takes the pointer only where it shows
// ticks. Its painter and hit test are bound once, so a frame allocates
// nothing for them, and the current turn is read from the list as it
// paints, after layout, so the highlight follows the frame it shows.
type navigation struct {
	c      *ui.Context
	list   *ui.ListState
	turns  []Turn
	hover  int     // the tick under the pointer, -1 for none
	drag   bool    // a press on a tick goes on
	went   int     // the turn the press showed last
	active int     // the current turn, as painted last
	width  float32 // the transcript's width, as painted last
	height float32
	tick   ui.Color
	muted  ui.Color
	// value is what assistive technology reads, "a/n", kept for
	// the turn and count it was written for, so a frame writes none.
	value   string
	valueOf [2]int

	paint func(*ui.Painter, ui.Rect)
	hit   func(x, y float32, r ui.Rect) bool
	build func()
}

// wide reports whether the rail shows at the width painted last.
func (u *navigation) wide() bool { return u.width >= railMin }

// view builds the rail over the pane, after the transcript and the composer.
func (u *navigation) view(c *ui.Context, l *ui.ListState, turns []Turn) {
	if u.paint == nil {
		u.paint, u.hit, u.build, u.went = u.draw, u.hits, u.children, -1
	}
	p := theme.Of(c)
	u.c, u.list, u.turns, u.tick, u.muted = c, l, turns, p.Foreground, p.MutedForeground
	ui.Box(c.Key("navigation")).Absolute().Left(0).Top(0).Right(0).Bottom(0).PassThrough().
		Draw(u.paint).Children(u.build)
}

func (u *navigation) children() {
	c, n := u.c, len(u.turns)
	wide := u.wide()
	rail := ui.Box(c.Key("rail")).Absolute().Left(0).Top(0).Bottom(0).Width(railWidth).HitTest(u.hit).
		Role(ui.RoleSlider).Label(i18n.T(c, "chat_navigation")).Value(u.valueText(min(u.active, n-1)+1, n)).
		Focusable().TabStop(wide).AccessHidden(!wide)
	x, y, over := rail.PointerPosition()
	pressed := rail.Pressed()
	hover := -1
	switch {
	case !wide:
	case pressed && u.drag:
		hover = indexAt(y, u.height, n)
	case over:
		hover = u.at(x, y, n)
	}
	if !pressed {
		u.drag, u.went = false, -1
	} else if !u.drag && hover >= 0 {
		u.drag = true
	}
	u.hover = hover
	if u.drag && hover >= 0 && hover != u.went {
		u.went = hover
		u.navigate(hover)
	}
	if wide {
		last := n - 1
		switch {
		case rail.Shortcut(0, ui.KeyUp):
			u.navigate(max(u.active-1, 0))
		case rail.Shortcut(0, ui.KeyDown):
			u.navigate(min(u.active+1, last))
		case rail.Shortcut(0, ui.KeyHome):
			u.navigate(0)
		case rail.Shortcut(0, ui.KeyEnd):
			u.navigate(last)
		}
	}
	if hover < 0 || kit.TipsHidden(c) {
		return
	}
	// The preview stands beside the hovered tick, as Kit's tooltip at
	// the tick's place on the track rather than at the pointer.
	anchor := ui.Box(c.Key("tick")).Absolute().Left(trackPad).Top(tickY(u.height, n, hover)).
		Width(railWidth - trackPad).Height(0).PassThrough()
	title := u.title(hover)
	t := c.Theme()
	ui.Overlay(c, func() {
		tip := ui.Column(c)
		kit.Anchored(tip, anchor, kit.TipLayer)
		kit.Attach(kit.Floating(c, tip), anchor, kit.Right, 8)
		tip.Radius(theme.Of(c).Radius).Width(previewW+2*t.Space(2)).Padding(t.Space(1.25), t.Space(2)).FontSize(t.FontSize - 1).
			Children(func() { ui.Text(c, title) })
	})
}

// valueText is "i/n", written again only as either changes.
func (u *navigation) valueText(i, n int) string {
	if u.valueOf != [2]int{i, n} {
		u.value, u.valueOf = strconv.Itoa(i)+"/"+strconv.Itoa(n), [2]int{i, n}
	}
	return u.value
}

// navigate shows turn i at the top of the view.
func (u *navigation) navigate(i int) {
	if i < 0 || i >= len(u.turns) {
		return
	}
	u.active = i
	u.list.ScrollTo(i, ui.Start)
}

// current is the turn at the top of the view, the last while the list
// shows its end, or the one shown last before the list is laid out.
func (u *navigation) current() int {
	n := len(u.turns)
	if u.list.AtEnd() {
		return n - 1
	}
	if first, last := u.list.Visible(); last >= first {
		return first
	}
	return min(u.active, n-1)
}

// draw records the transcript's size and paints the ticks.
func (u *navigation) draw(pt *ui.Painter, r ui.Rect) {
	u.width, u.height = r.W, r.H
	n := len(u.turns)
	if !u.wide() || n == 0 {
		return
	}
	cur := u.current()
	u.active = cur
	x := r.X + trackPad + tickInset
	// Ticks closer than their height overlap into a band, as a long
	// conversation's do. A run of plain ticks there (not current, too far
	// from the hover to widen) paints as one rect in the alpha the run's
	// overlapping ticks add up to, rather than a fill per turn.
	spacing := min(r.H, float32(n)*tickSlot) / float32(n)
	band, from := ui.Color{}, -1
	if spacing < tickHeight {
		band = u.muted.Alpha(float32(1 - math.Pow(1-0.32, float64(tickHeight/spacing))))
	}
	flush := func(to int) {
		if from >= 0 {
			y := r.Y + tickY(r.H, n, from)
			pt.Fill(ui.Rect{X: x, Y: y, W: 5, H: r.Y + tickY(r.H, n, to-1) + tickHeight - y}, band, 0)
			from = -1
		}
	}
	for i := range n {
		near := u.hover >= 0 && i-u.hover <= tickReach && u.hover-i <= tickReach
		if band.A != 0 && i != cur && !near {
			if from < 0 {
				from = i
			}
			continue
		}
		flush(i)
		s := strength(i, u.hover)
		col := u.tick
		if i != cur {
			col = u.muted.Alpha(0.32 + s*0.46)
		}
		pt.Fill(ui.Rect{X: x, Y: r.Y + tickY(r.H, n, i), W: 5 + 25*s, H: tickHeight}, col, 0)
	}
	flush(n)
}

// hits reports whether (x, y) is on a tick, or near one, rather than on
// the empty rail.
func (u *navigation) hits(x, y float32, r ui.Rect) bool {
	return u.wide() && u.at(x-r.X, y-r.Y, len(u.turns)) >= 0
}

// at returns the tick at (x, y) in the rail, -1 for none: a small margin
// keeps thin ticks usable without taking the empty rail.
func (u *navigation) at(x, y float32, n int) int {
	h := u.height
	if n == 0 || h <= 0 || x < trackPad || x > railWidth || y < 0 || y > h {
		return -1
	}
	i := indexAt(y, h, n)
	dx := x - trackPad - tickInset
	dy := y - tickY(h, n, i)
	w := 5 + 25*strength(i, u.hover)
	if dx < -tickHit || dx > w+tickHit || abs(dy) > min(h/float32(n), tickSlot)/2+1e-3 {
		return -1
	}
	return i
}

// title is turn i's first line of text, or the new chat's label.
func (u *navigation) title(i int) string {
	text := u.turns[i].User
	for text != "" {
		line, rest, _ := strings.Cut(text, "\n")
		if line = strings.TrimSpace(line); line != "" {
			return line
		}
		text = rest
	}
	return i18n.T(u.c, "chat_new")
}

// indexAt returns the tick at y on a track h tall.
func indexAt(y, h float32, n int) int {
	if n == 0 || h <= 0 {
		return 0
	}
	extent := min(h, float32(n)*tickSlot)
	i := int(max(0, float32(math.Floor(float64((y-(h-extent)/2)/extent*float32(n))))))
	return min(i, n-1)
}

// tickY is the top of tick i on a track h tall.
func tickY(h float32, n, i int) float32 {
	extent := min(h, float32(n)*tickSlot)
	return (h-extent)/2 + extent*(float32(i)+0.5)/float32(max(n, 1))
}

// strength is how far tick i widens toward the hovered one, from 0 to 1.
func strength(i, hover int) float32 {
	if hover < 0 {
		return 0
	}
	d := float64(i - hover)
	return float32(math.Exp(-d * d / 12))
}

func abs(v float32) float32 {
	if v < 0 {
		return -v
	}
	return v
}
