package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The session around the turns: the first page loading, older pages loading
// as the start comes into view, the connection's banner while it recovers, a
// diagnostic of a failure, and the selection's Quote and Copy.

// loading is the transcript before its first page: a loader and its label.
func loading(c *ui.Context) {
	p := theme.Of(c)
	ui.Column(c.Key("loading")).Grow(1).Center().Gap(12).Children(func() {
		kit.Loader(c, 20)
		ui.Text(c, i18n.T(c, "chat_history_loading")).FontSize(14).TextColor(p.MutedForeground)
	})
}

// older is the row before the first turn while there is history before it: a
// loader while the page loads, its failure as a Result with Retry, or
// nothing, as reaching it asks for the page.
func (s *State) older(c *ui.Context, sess *Session, actions Actions) {
	if sess == nil || sess.Older == 0 {
		return
	}
	if sess.OlderFailed {
		kit.Result(c.Key("older"), kit.ResultProps{Status: kit.ResultError, Title: i18n.T(c, "chat_history_failed"), Extra: func() {
			kit.Ghost(c.Key("retry"), i18n.T(c, "chat_retry"), false).Height(28).PaddingX(8).
				Children(func() { ui.Text(c, i18n.T(c, "chat_retry")).FontSize(14) }).OnClick(actions.LoadOlder)
		}})
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("older")).Height(32).Gap(8).Justify(ui.Center).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
		kit.Loader(c, 14)
		if !sess.LoadingOlder {
			// The start came into view: ask once this frame is built.
			s.wantOlder = true
		}
	})
}

// banner tells the connection is recovering or lost: the last row of the
// transcript, after the last turn. It is the session's state, not a message,
// and offers nothing: the Client reconnects by itself (AGENTS.md, Removed UI).
func banner(c *ui.Context, sess *Session) {
	if sess == nil || sess.Connection == Connected {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("banner")).FillWidth().MaxWidth(theme.ContentWidth).Margin(8, ui.Auto, 0, ui.Auto).Padding(10, 12).Gap(8).Shrink(0).
		Radius(p.RadiusLg).Background(p.Floating()).Border(1, p.Border).AlignItems(ui.Center).Role(ui.RoleStatus).Children(func() {
		if sess.Connection == Reconnecting {
			kit.Loader(c, 14)
			kit.Shimmer(c, i18n.T(c, "chat_reconnecting"), 14, p.MutedForeground).Grow(1)
		} else {
			ui.Icon(c, icons.TriangleAlert).FontSize(16).TextColor(p.Danger)
			ui.Text(c, i18n.T(c, "plugins_view_offline")).FontSize(14).Grow(1).TextColor(p.Danger)
		}
	})
}

// diagnostic is a failure in detail after the last turn: its heading,
// each field as a label and value, and the raw message, with Copy.
func diagnostic(c *ui.Context, d *Diagnostic) {
	if d == nil {
		return
	}
	p := theme.Of(c)
	size := textSize(c)
	ui.Column(c.Key("diagnostic")).FillWidth().Padding(12).Gap(8).Radius(p.RadiusLg).Border(1, p.Danger.Alpha(0.4)).
		Background(p.Danger.Alpha(0.06)).Children(func() {
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Icon(c, icons.TriangleAlert).FontSize(rowIcon(size)).TextColor(p.Danger)
			ui.Text(c, d.Title).FontSize(size).FontWeight(500).Grow(1).TextColor(p.Danger)
			kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "content_copy"), 28, false).OnClick(func() {
				c.WriteClipboard(DiagnosticText(d))
				kit.Message(c, i18n.T(c, "content_copied"))
			})
		})
		for i, f := range d.Fields {
			ui.Row(c.Key(i)).Gap(12).Children(func() {
				ui.Text(c, f[0]).FontSize(size - 1).Width(120).Shrink(0).TextColor(p.MutedForeground)
				ui.Text(c, f[1]).Selectable().Font("monospace").FontSize(size - 1).Grow(1).Shrink(1).MinWidth(0)
			})
		}
		if d.Detail != "" {
			ui.Scroll(c).MaxHeight(120).Children(func() {
				ui.Text(c, d.Detail).Selectable().Font("monospace").FontSize(size - 2).TextColor(p.MutedForeground)
			})
		}
	})
}

// DiagnosticText is what Copy copies of a diagnostic: the heading, the
// fields as "label: value" lines, and the message.
func DiagnosticText(d *Diagnostic) string {
	var b strings.Builder
	b.WriteString(d.Title)
	for _, f := range d.Fields {
		fmt.Fprintf(&b, "\n%s: %s", f[0], f[1])
	}
	if d.Detail != "" {
		b.WriteString("\n\n" + d.Detail)
	}
	return b.String()
}

// Quote is a selection as the composer quotes it: each line after "> ", then
// a blank line.
func Quote(text string) string {
	lines := strings.Split(strings.TrimRight(text, "\n"), "\n")
	for i, l := range lines {
		lines[i] = "> " + l
	}
	return strings.Join(lines, "\n") + "\n\n"
}

// press is the pointer's primary button over the transcript, as its pane
// observes it: MyGo tells no element whether a button is held, and the
// paragraph a selection drags from takes the press.
type press struct {
	held, away bool
	downs, ups int // presses and releases seen, for the turns to tell a new one
}

// watch observes the presses and releases over pane. A release outside it
// goes unseen: the press counts as let go once the pointer is back over the
// pane after leaving it.
func (p *press) watch(pane ui.Element) {
	pane.ObserveInput(func(ev ui.InputEvent) {
		if ev.Button != 0 {
			return
		}
		switch ev.Kind {
		case ui.InputPointerDown:
			p.held, p.away = true, false
			p.downs++
		case ui.InputPointerUp:
			p.held = false
			p.ups++
		}
	})
	if !p.held {
		return
	}
	switch _, _, over := pane.PointerPosition(); {
	case !over:
		p.away = true
	case p.away:
		p.held, p.away = false, false
		p.ups++
	}
}

// textSpan is a box of a turn that holds selectable text, as of the last
// frame in window DIPs: the user bubble's text, with its paragraph's id, or
// a reply's Markdown (id 0, its paragraphs being MyGo's).
type textSpan struct {
	id uint64
	r  ui.Rect
}

// span notes el as a text box of the turn being built, inset by its padding.
func (s *State) span(el ui.Element, id uint64, padY, padX float32) {
	r := el.Bounds()
	if r.W <= 2*padX || r.H <= 2*padY {
		return
	}
	s.spans = append(s.spans, textSpan{id, ui.Rect{X: r.X + padX, Y: r.Y + padY, W: r.W - 2*padX, H: r.H - 2*padY}})
}

// selectionAt is a turn's selection and where its pill stands, in the turn's
// own coordinates so that it moves with the text.
type selectionAt struct {
	text           string
	press, release ui.Point // where the pointer began and ended the selection
	downs, ups     int      // the presses and releases this turn has seen
	onPill         bool     // the press under way is on the pill
	keyboard       bool     // the selection was made without the pointer
	pointed        bool     // the pointer let go and no key followed: a change is its own
	hidden         bool     // dismissed until the selection changes
	offset         float32  // the turn's place in the visible area
	seen           bool
	pill           ui.Rect // the pill as placed last; W and H as measured
	placed         bool
}

type selectionKey struct{}

// selectionGap is between the pill and the selection, the reference's.
const selectionGap = 8

// selection is the pill of a turn holding a selection: Quote (disabled in a
// read-only session), Copy and New conversation.
//
// It shows once the pointer lets go, above the selection and centred on it,
// below it where the turn or the visible transcript has no room above, and
// inside both; never over the selected lines unless they fill the view. A
// press elsewhere, Escape, one of its actions or the transcript scrolling
// dismisses it until the selection changes.
//
// MyGo tells the selected paragraphs' ids and text, not where they are, so
// the selection's place is taken from the pointer: the lines between where
// the press began and where it ended, a line's height around each. The text
// boxes the turn noted correct a pointer that left the text, which selects
// to the nearest paragraph: a selection inside the user bubble stays in the
// bubble's own box, and an end outside every box widens the lines to the
// nearest box's edge. A selection made with the keyboard takes the boxes
// themselves.
func (s *State) selection(c *ui.Context, session string, turn ui.Element, readonly bool, actions Actions) {
	at := ui.Local(turn, selectionKey{}, func() selectionAt { return selectionAt{downs: s.press.downs, ups: s.press.ups} })
	turn.ObserveInput(func(ev ui.InputEvent) {
		if ev.Kind != ui.InputKeyDown {
			return
		}
		at.pointed = false
		if ev.Key == ui.KeyEscape {
			at.hidden = true
		}
	})
	if at.downs != s.press.downs {
		at.downs = s.press.downs
		x, y, _ := turn.PointerPosition()
		at.onPill = at.placed && x >= at.pill.X && x < at.pill.X+at.pill.W && y >= at.pill.Y && y < at.pill.Y+at.pill.H
		if !at.onPill {
			// Shift extends the selection from where it began.
			if at.text == "" || c.Modifiers()&ui.Shift == 0 {
				at.press = ui.Point{X: x, Y: y}
			}
			at.hidden = at.text != ""
		}
	}
	released := at.ups != s.press.ups
	at.ups = s.press.ups
	if released && !at.onPill {
		at.pointed = true
	}

	sel := turn.SelectedTexts()
	var parts []string
	for _, t := range sel {
		r := []rune(t.Text)
		parts = append(parts, string(r[min(t.Start, len(r)):min(t.End, len(r))]))
	}
	text := strings.Join(parts, "\n")
	if strings.TrimSpace(text) == "" {
		at.text, at.hidden, at.keyboard, at.seen, at.placed = "", false, false, false, false
		return
	}
	held := s.press.held && !at.onPill
	if text != at.text {
		at.text, at.hidden = text, false
		// MyGo tells the selection as of the last frame: a quick drag's shows
		// in the frame after the pointer let go.
		at.keyboard = !held && !at.pointed
	}
	if (held || released) && !at.onPill {
		x, y, _ := turn.PointerPosition()
		at.release = ui.Point{X: x, Y: y}
	}
	box, area := turn.Bounds(), s.area[session]
	if box.W <= 0 || box.H <= 0 {
		return
	}
	if area.W <= 0 || area.H <= 0 {
		area = box
	}
	// The transcript moving under a pill that shows dismisses it; not the
	// last step of a drag that scrolled it, seen as the pointer lets go.
	if offset := box.Y - area.Y; at.seen && !held && !released && (offset-at.offset > 0.5 || at.offset-offset > 0.5) {
		at.hidden = true
		at.offset = offset
	} else {
		at.offset, at.seen = offset, true
	}
	if held || at.hidden {
		at.placed = false
		return
	}

	// The selected lines, in the turn.
	line := textSize(c) * 1.75
	top, bottom := min(at.press.Y, at.release.Y)-line, max(at.press.Y, at.release.Y)+line
	left, right := min(at.press.X, at.release.X), max(at.press.X, at.release.X)
	var bubble *ui.Rect
	var bubbleID uint64
	var texts []ui.Rect // the other boxes
	for _, sp := range s.spans {
		r := sp.r
		r.X, r.Y = r.X-box.X, r.Y-box.Y
		if sp.id != 0 {
			bubble, bubbleID = &r, sp.id
		} else {
			texts = append(texts, r)
		}
	}
	inBubble, onlyBubble := false, bubble != nil
	for _, t := range sel {
		same := bubble != nil && bubbleID == t.ID
		inBubble, onlyBubble = inBubble || same, onlyBubble && same
	}
	switch {
	case onlyBubble:
		if at.keyboard || top >= bubble.Y+bubble.H || bottom <= bubble.Y {
			top, bottom = bubble.Y, bubble.Y+bubble.H
		}
		top, bottom = max(top, bubble.Y), min(bottom, bubble.Y+bubble.H)
		left, right = bubble.X, bubble.X+bubble.W
	case at.keyboard:
		// Everything the turn's boxes hold, as Select All takes it.
		top, bottom, left, right = 0, box.H, 0, box.W
		for i, r := range texts {
			if i == 0 {
				top, bottom, left, right = r.Y, r.Y+r.H, r.X, r.X+r.W
			}
			top, bottom = min(top, r.Y), max(bottom, r.Y+r.H)
			left, right = min(left, r.X), max(right, r.X+r.W)
		}
		if inBubble {
			top = bubble.Y
		}
	default:
		for _, y := range []float32{at.press.Y, at.release.Y} {
			// An end outside every box selected to the nearest one's edge.
			var near *ui.Rect
			distance := float32(0)
			for i, r := range texts {
				d := max(r.Y-y, y-r.Y-r.H, 0)
				if near == nil || d < distance {
					near, distance = &texts[i], d
				}
			}
			switch {
			case near == nil || distance == 0:
			case near.Y > y:
				bottom = max(bottom, near.Y+line)
			default:
				top = min(top, near.Y+near.H-line)
			}
		}
		if bubble != nil {
			if edge := bubble.Y + bubble.H; inBubble {
				top = max(top, bubble.Y)
			} else if edge < bottom {
				top = max(top, edge)
			}
		}
	}
	top, bottom = max(top, 0), min(bottom, box.H)

	// The room: the visible transcript down to the turn's end. The pill may
	// stand over the turn before, which paints under it, not the one after.
	roomTop, roomBottom := area.Y-box.Y, min(area.Y+area.H-box.Y, box.H)
	roomLeft, roomRight := max(area.X-box.X, 0), min(area.X+area.W-box.X, box.W)
	pill := kit.Floating(c, ui.Row(c.Key("selection"))).Absolute().Padding(4).Gap(4).Radius(18)
	pill.Children(func() {
		button := func(key, label string, do func()) ui.Element {
			return kit.Ghost(c.Key(key), label, false).Height(28).PaddingX(10).Radius(14).
				Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() {
				do()
				at.hidden = true
			})
		}
		button("quote", i18n.T(c, "message_quote"), func() { actions.Quote(Quote(text)) }).Disabled(readonly)
		button("copy", i18n.T(c, "message_copy"), func() {
			c.WriteClipboard(text)
			kit.Message(c, i18n.T(c, "content_copied"))
		})
		button("new", i18n.T(c, "chat_new"), func() { actions.Start(Quote(text)) })
	})
	// Laid out, then placed: its width follows its labels.
	if size := pill.Bounds(); size.W > 0 && size.H > 0 {
		at.pill.W, at.pill.H = size.W, size.H
	}
	w, h := at.pill.W, at.pill.H
	if w == 0 || min(bottom, roomBottom) <= max(top, roomTop) {
		// Not measured yet, or the selection is out of view.
		pill.Top(0).Left(0).Invisible().IgnoreInput(true)
		at.placed = false
		if w == 0 {
			c.Invalidate()
		}
		return
	}
	y := top - selectionGap - h
	if y < roomTop {
		y = bottom + selectionGap
	}
	if y+h > roomBottom {
		// The selection fills the view: the pill stays in it, at its top.
		y = min(roomTop+selectionGap, roomBottom-h)
	}
	x := max(min((left+right-w)/2, roomRight-w), roomLeft)
	pill.Top(y).Left(x)
	at.pill.X, at.pill.Y, at.placed = x, y, true
	// A press the pane does not see, as on the composer, dismisses it too.
	if pill.PressedOutside() {
		at.hidden = true
		c.Invalidate()
	}
}
