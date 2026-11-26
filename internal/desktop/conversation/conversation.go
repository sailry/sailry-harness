// Package conversation is the transcript of a conversation: a virtualized
// list of turns, each a user message and the assistant's reply with its work
// region, tool calls and Markdown answer. The view models and actions are in
// model.go.
package conversation

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Layout metrics.
const (
	padX       = 24
	turnGap    = 10 // user message to reply
	turnBottom = 28 // between turns
	bubbleMax  = 440
	userLift   = 0.06 // the user message's lift toward the text on a dark page
	workGap    = 16   // between the blocks of a reply

	userLeading    = 1.6 // the user message's line height, in ems
	collapsedLines = 5   // the lines a long user message shows until expanded
)

// State is the transcript's UI state: a list state per conversation, so
// each keeps its place, the disclosures opened and view-only mention bindings.
// Canonical conversation data remains in Client.
type State struct {
	toolCards     map[string]map[string]func(*ui.Context)
	copyCancel    context.CancelFunc
	renderSession string
	lists         map[string]*transcriptList
	boundLists    []*transcriptList
	anchors       map[string]transcriptAnchor
	anchorOrder   uint64
	open          map[string]bool                // the disclosures open
	openOwner     map[string]map[string]struct{} // open keys owned per session
	revealOpen    string                         // disclosure opened on the last click, to scroll into view
	revealWaiting bool                           // true while revealOpen awaits an async detail load
	work          map[string]bool                // work regions the user opened or closed
	workOwner     map[string]map[string]struct{} // work keys owned per session
	built         map[string]bool                // lists built once, so laid out
	presses       map[string]*press              // the pointer's button over each conversation's transcript
	spans         []textSpan                     // the turn being built's text boxes, for its selection pill
	area          map[string]ui.Rect             // each conversation's list above its dock: what shows of it
	docks         map[string]*dock               // what floats over each conversation's end (Docked)
	md            markdown.Options               // the replies' (kit.Markdown fills the rest)
	mdBase        *markdown.Theme
	mdSize        float32
	light         float32            // the welcome wordmark's beam, in window DIPs
	lit           bool               // the pointer has moved over the welcome page
	letters       lettering          // the welcome page's custom lettering laid out last
	views         map[string]*Viewer // the image viewer of each conversation
	edits         map[string]*edit   // the open message editor of each conversation
	rewind        string             // the turn whose rewind waits to be confirmed
	rewindOf      string             // the conversation it is of
	reading       string             // the turn read aloud
	finds         map[string]*search // the search bar of each conversation
	navigated     *transcriptList    // the transcript built last, when it has a rail not built yet
	// Split is whether the transcripts show in the panes of a split, which have
	// no navigation rail.
	Split bool
	// Mark is the welcome page's custom lettering (Settings › Appearance);
	// blank shows the Sailry wordmark.
	Mark string
	// Aside is the room Docked keeps clear at the transcript's end for what
	// floats there (the session overview card): the turns, the dock and the
	// rail center in what is left, while the list still fills the pane, so
	// its scroll bar stays at the pane's edge. The caller sets it per call.
	Aside           float32
	CompactDisplay  bool
	ReaderActive    func(string) bool
	resolveImage    func(string, Image) Image
	Search          func(string, string, *uint64, func(SearchPage)) func()
	Reveal          func(string, Match, func(int, bool)) func()
	references      map[string]map[string][]Reference
	referenceDetail referenceDetail
	// OpenLink takes a link or a file path of the conversation's content
	// the application opens itself: in the right panel on a click, with the
	// system on a click with the platform modifier (External). It reports
	// false for one it leaves to the transcript: a heading anchor scrolls,
	// and an address ui.SafeToOpen allows opens in the system.
	OpenLink func(c *ui.Context, session, link string) bool
	// Embed resolves image markup (image true), or a link, standing alone in
	// a reply of session to its picture or file card. A link resolves only
	// to a deliverable or a picture; false leaves it as Markdown shows it.
	Embed func(session, dest, label string, image bool) (Embed, bool)
	// SessionLink reports a link that opens a session: it carries the
	// agent's icon.
	SessionLink func(link string) bool
	result      *Block            // the search result open over the window
	resultOf    string            // the conversation it is of
	resultMD    *markdown.Options // its answer's Markdown
}

// External reports whether the click being handled opens its target with
// the system rather than in the right panel: the platform modifier is held
// (Command on macOS, Control elsewhere).
func External(c *ui.Context) bool { return c.Modifiers()&ui.Cmd != 0 }

// follow opens a link or a file path of session's content, and reports
// whether the application took it (OpenLink).
func (s *State) follow(c *ui.Context, session, link string) bool {
	switch {
	case s.OpenLink != nil && s.OpenLink(c, session, link):
		return true
	case strings.HasPrefix(link, "#"):
		// OnLink takes every link, so a heading link scrolls here.
		s.md.Anchor = strings.TrimPrefix(link, "#")
		s.md.AnchorVersion++
	case ui.SafeToOpen(link):
		c.OpenURL(link)
	}
	return false
}

// pathLink is a file's path as the link OpenLink takes: the characters a
// link reads as its query, fragment or escapes are escaped.
func pathLink(path string) string {
	return strings.NewReplacer("%", "%25", "#", "%23", "?", "%3F").Replace(path)
}

func New() *State {
	return &State{lists: map[string]*transcriptList{}, open: map[string]bool{}, work: map[string]bool{}, built: map[string]bool{}, area: map[string]ui.Rect{}, docks: map[string]*dock{}}
}

// Forget releases retained per-session transcript state after a session is
// deleted or a local draft is discarded.
func (s *State) Forget(id string) {
	delete(s.anchors, id)
	delete(s.edits, id)
	delete(s.references, id)
	if s.rewindOf == id {
		s.rewind, s.rewindOf = "", ""
	}
	for key := range s.openOwner[id] {
		delete(s.open, key)
		if s.revealOpen == key {
			s.revealOpen = ""
			s.revealWaiting = false
		}
	}
	delete(s.openOwner, id)
	for key := range s.workOwner[id] {
		delete(s.work, key)
	}
	delete(s.workOwner, id)
}

func (s *State) setOpen(session, key string, open bool) {
	if open {
		s.open[key] = true
		if session != "" {
			if s.openOwner == nil {
				s.openOwner = map[string]map[string]struct{}{}
			}
			keys := s.openOwner[session]
			if keys == nil {
				keys = map[string]struct{}{}
				s.openOwner[session] = keys
			}
			keys[key] = struct{}{}
		}
		return
	}
	delete(s.open, key)
	if keys := s.openOwner[session]; keys != nil {
		delete(keys, key)
		if len(keys) == 0 {
			delete(s.openOwner, session)
		}
	}
}

func (s *State) setWork(session, key string, open bool) {
	s.work[key] = open
	if session != "" {
		if s.workOwner == nil {
			s.workOwner = map[string]map[string]struct{}{}
		}
		keys := s.workOwner[session]
		if keys == nil {
			keys = map[string]struct{}{}
			s.workOwner[session] = keys
		}
		keys[key] = struct{}{}
	}
}

// Navigation builds the message navigation of the transcript View built
// last, over the element it is called in: the pane that holds the transcript
// and what is under it, the composer, so the ticks centre in the pane's
// height rather than in the list's. It builds nothing for a transcript
// without one.
func Navigation(c *ui.Context, s *State) {
	if l := s.navigated; l != nil {
		s.navigated = nil
		if l.nav == nil {
			l.nav = &navigation{}
		}
		l.nav.view(c, l)
	}
}

// pressOf returns the press tracker of a conversation: two transcripts on
// screen each watch their own pane.
func (s *State) pressOf(id string) *press {
	p := s.presses[id]
	if p == nil {
		p = &press{}
		if s.presses == nil {
			s.presses = map[string]*press{}
		}
		s.presses[id] = p
	}
	return p
}

// viewOf returns the image viewer of a conversation, so the transcript
// that opened it builds it and takes its actions.
func (s *State) viewOf(id string) *Viewer {
	v := s.views[id]
	if v == nil {
		v = &Viewer{}
		if s.views == nil {
			s.views = map[string]*Viewer{}
		}
		s.views[id] = v
	}
	return v
}

// View builds the transcript of conversation id: its turns in a list that
// follows its end, the session's states around them, the search bar and
// the jump button over it, and the image viewer and rewind confirmation
// over the window. sess may be nil.
func View(c *ui.Context, s *State, id string, turns []Turn, sess *Session, actions Actions) {
	Docked(c, s, id, turns, sess, actions, nil)
}

// dock is what floats over a transcript's end: its element, for its height
// as the last frame laid it out, and the check that the frame left that
// much room, bound once.
type dock struct {
	box     ui.Handle
	c       *ui.Context
	reserve float32
	check   func(*ui.Painter, ui.Rect)
}

// settle asks for another frame when the dock was laid out at another height
// than the list left room for, so the end rests on the dock in the frame
// after it grew or shrank.
func (d *dock) settle(*ui.Painter, ui.Rect) {
	if h := d.box.Bounds(d.c).H - d.reserve; h > 0.5 || h < -0.5 {
		d.c.Invalidate()
	}
}

// Docked is View with below, the composer and what goes with it, floating
// over the transcript's end rather than under the transcript: the list fills
// the pane, so its scroll bar runs the pane's height at the pane's edge and
// the turns pass behind what floats. The list ends with room as tall as the
// dock was laid out last, so its end rests on the dock, and follows it as the
// dock grows or shrinks while the list shows its end. Beside the content
// column the dock lets the pointer through: the wheel and presses there are
// the transcript's. The turns fade out as they reach the dock and do not
// show behind it (fadeUnder), so the dock needs no sheet of its own. A
// session still loading has below under its loader.
func Docked(c *ui.Context, s *State, id string, turns []Turn, sess *Session, actions Actions, below func()) {
	previousSession := s.renderSession
	s.renderSession = id
	defer func() { s.renderSession = previousSession }()
	s.messageTheme(c)
	s.md.ShareSelection = true
	kit.BindMermaid(&s.md, drawMermaidChart)
	bindMath(&s.md)
	s.md.LinkIcon, s.md.Preview = nil, nil
	if sessionLink := s.SessionLink; sessionLink != nil {
		s.md.LinkIcon = sessionIcon(c, sessionLink)
		s.md.Preview = func(c *ui.Context, dest string) func(ui.Element) {
			if !sessionLink(dest) {
				return nil
			}
			return func(tip ui.Element) {
				t := c.Theme()
				kit.Floating(c, tip).Radius(theme.Of(c).Radius).
					MaxWidth(t.Space(80)).Padding(t.Space(1.25), t.Space(2)).FontSize(t.FontSize-1).
					Margin(0, 0, t.Space(1.5), 0).
					Children(func() { ui.Text(c, i18n.T(c, "notification_open_conversation")) })
			}
		}
	}
	if sess != nil && sess.Loading {
		s.navigated = nil
		loading(c)
		if below != nil {
			below()
		}
		return
	}
	var d *dock
	var reserve float32
	if below != nil {
		d = s.docks[id]
		if d == nil {
			d = &dock{}
			d.check = d.settle
			s.docks[id] = d
		}
		reserve = d.box.Bounds(c).H
		d.c, d.reserve = c, reserve
	}
	n := len(turns)
	// A lost or recovering connection is told after the last turn, where the
	// next reply would come; a session without turns has it over the list.
	if n == 0 {
		banner(c, sess)
	}
	// The dock is not in the box that watches the transcript's presses: a
	// press on it is one the transcript does not see.
	pane := ui.Box(c.Key(id)).Grow(1)
	if d == nil {
		s.transcript(c, pane, id, turns, sess, actions, 0)
	} else {
		pane.DrawOver(d.check).Children(func() {
			s.transcript(c, ui.Box(c.Key("transcript")).Fill(), id, turns, sess, actions, reserve)
			ui.Column(c.Key("dock")).Absolute().Left(0).Right(s.Aside).Bottom(0).PassThrough().Bind(&d.box).Children(func() {
				ui.Column(c.Key("sheet")).FillWidth().MaxWidth(theme.ContentWidth).Margin(0, ui.Auto).Children(below)
			})
		})
	}
	s.viewer(c, id, actions, sess)
	s.confirmRewind(c, id, actions)
	s.searchResult(c, id)
	s.referenceDialog(c, id)
}

// transcript fills pane with the list of turns, the jump button and the
// search bar, over the theme's conversation effects. reserve is the room the
// list leaves under its last turn: the height of what floats over its end.
func (s *State) transcript(c *ui.Context, pane ui.Element, id string, turns []Turn, sess *Session, actions Actions, reserve float32) {
	kit.Draws(pane, kit.Backdrop(c, "conversation"))
	s.pressOf(id).watch(pane)
	pane.Children(func() {
		l, n := s.bindTranscript(c, id, turns, sess, actions), len(turns)
		wasVisible := l.olderVisible
		l.olderVisible = false
		if sess != nil && sess.Older > 0 && n > 0 && l.olderTurn == turns[0].ID {
			// MyGo builds rows outside the viewport, and one turn can be much taller
			// than the entire pane. Request history only when its marker is on screen.
			area := s.area[id]
			first, last := l.layout.Visible()
			marker := l.olderMarker.Bounds(c)
			l.olderVisible = first == 0 && last >= first && area.H > 0 && marker.H > 0 &&
				marker.Y < area.Y+area.H && marker.Y+marker.H > area.Y
			if l.olderVisible && !sess.LoadingOlder && (!sess.OlderFailed || !wasVisible || l.olderRetry) {
				actions.LoadOlder()
			}
		}
		l.olderRetry = false
		// What shows of the transcript is the list above the dock.
		list := ui.List(c, &l.layout, n, l.row).TrackScroll(&l.scroll).Fill().Padding(0, s.Aside, reserve, 0).Role(ui.RoleLog).Draw(l.paint)
		if l.latest != nil || l.jumping || l.olderVisible && sess.OlderFailed && !sess.LoadingOlder {
			list.HandleInput(l.input)
		}
		area := list.Bounds()
		fadeUnder(list, area, reserve)
		area.H = max(area.H-reserve, 0)
		s.area[id] = area

		// A delegated session's transcript, the child panel's (no session), is
		// read-only and has no navigation. The pane builds it (Navigation).
		s.navigated = nil
		if sess != nil && n > 0 && !s.Split {
			s.navigated = l
		}
		if l.jumping && l.layout.AtEnd() {
			l.jumping = false
			if source, ok := actions.(interface{ ReturnToLatest(context.Context) }); ok {
				ctx, cancel := context.WithCancel(context.Background())
				l.latest = cancel
				source.ReturnToLatest(ctx)
			}
		} else if l.latest != nil && !l.layout.AtEnd() {
			l.cancelLatest()
		}
		jump(c, &l.layout, n, !s.built[id], reserve, s.Aside, l.jump)
		s.built[id] = true
		s.searchBar(c, id, turns)
	})
}

// dockFade is how far over the dock a turn fades out, within the room the
// last turn keeps over the list's end, so nothing fades at the end.
const dockFade = 24

// fadeUnder masks the list, laid out at area, so its turns fade out as they
// reach what floats over its last reserve DIPs and do not show behind it.
// The strip at the trailing edge stays whole: the scroll bar runs the pane's
// height, beside the dock.
func fadeUnder(list ui.Element, area ui.Rect, reserve float32) {
	if reserve <= 0 || area.H <= reserve || area.W <= scrollStrip {
		return
	}
	// Keep the source layer when only the controls over the transcript change.
	// MyGo redraws it when the list's scene changes, including scroll and animation.
	list.Effect(dockFadeEffect{reserve: reserve}).Cache()
}

// scrollStrip is the width at the list's trailing edge the fade leaves
// whole, the scroll bar's; a turn's side padding is wider.
const scrollStrip = 16

func (s *State) turn(c *ui.Context, session string, i int, t *Turn, n int, sess *Session, actions Actions) ui.Element {
	first, last := i == 0, i == n-1
	top, bottom := float32(0), float32(turnBottom)
	if first {
		top = 24
	}
	if last {
		bottom = 24
	}
	q, current, marked := s.highlight(session, i, t)
	col := ui.Column(c).FillWidth().MaxWidth(theme.ContentWidth).Margin(0, ui.Auto).Padding(top, padX, bottom, padX).Gap(turnGap).Selectable()
	s.spans = s.spans[:0]
	col.Children(func() {
		// The pill is built last, over the turn and knowing its text boxes.
		// The session's state follows the last turn whatever it is, a
		// compaction too.
		defer func() {
			if last && sess != nil {
				s.diagnostic(c, sess.Diagnostic)
				banner(c, sess)
			}
			s.selection(c, session, col, sess != nil && sess.ReadOnly, actions)
		}()
		if first {
			s.older(c, session, t.ID, sess)
		}
		if t.Compaction != nil {
			s.compaction(c, t)
			return
		}
		// Guidance the user sent into a continued turn still shows below.
		if t.Marker != nil {
			marker(c, t.Marker)
		}
		mark := ""
		if marked {
			mark = q
		}
		if len(t.Segments) == 0 {
			s.user(c, session, t, q, current, marked, actions)
			if !t.InputOnly {
				s.reply(c, t, mark, actions)
			}
			return
		}
		blocks := cited(t.Reply.Blocks, t.Reply.Citations)
		for index, segment := range t.Segments {
			key := fmt.Sprintf("%s-segment-%d", t.ID, index)
			ui.Column(c.Key(key)).FillWidth().Gap(turnGap).Children(func() {
				for inputIndex, input := range segment.Inputs {
					message := Turn{ID: input.ID, User: input.User, Sent: input.Sent, Controls: t.Controls}
					if message.ID == t.ID {
						message.Editing = t.Editing
					} else {
						message.Controls.NoEdit = true
					}
					for _, image := range input.Images {
						message.Images = append(message.Images, t.Images[image])
					}
					for _, file := range input.Files {
						message.Files = append(message.Files, t.Files[file])
					}
					ui.Column(c.Key(inputIndex)).FillWidth().Children(func() {
						s.user(c, session, &message, q, current, marked, actions)
					})
					if marked {
						current -= strings.Count(strings.ToLower(input.User), strings.ToLower(q))
					}
				}
				end := len(blocks)
				latest := index == len(t.Segments)-1
				if !latest {
					end = t.Segments[index+1].From
				}
				if latest || end > segment.From {
					s.replyPart(c, t, blocks[segment.From:end], mark, actions, latest, key)
				}
			})
		}
	})
	return col
}

// user renders one original or consumed input with its own identity.
func (s *State) user(c *ui.Context, session string, t *Turn, q string, current int, marked bool, actions Actions) {
	if t.User == "" && len(t.Images) == 0 && len(t.Files) == 0 {
		return
	}
	col := ui.Column(c.Key("user")).Gap(8)
	over := col.Hovered()
	col.Children(func() {
		var fold userFold
		if t.Editing != nil {
			s.editor(c, session, t, actions)
		} else if t.User != "" {
			var hl []ui.Span
			// The search's current match shows even where it is in the lines
			// a collapsed message hides; the message folds again after it.
			reveal := false
			if marked {
				hl = spans(t.User, q, current, theme.Of(c))
				reveal = current >= 0 && current < len(hl)/2
			}
			fold.key = "user-more-" + t.ID
			fold.open = s.opened(fold.key)
			text, shown, long := userMessage(c, t.User, hl, s.references[session][t.ID], fold.open || reveal)
			fold.long = long && !reveal
			s.span(shown, text.ID(), 0, 14)
		}
		if len(t.Images) > 0 {
			s.thumbnails(c, "user-images", t.Images, true)
		}
		if len(t.Files) > 0 {
			ui.Row(c).Justify(ui.End).Children(func() {
				ui.Column(c).Width(bubbleMax).Children(func() { s.files(c, "user-files", t.Files, actions) })
			})
		}
		s.userFooter(c, t, over, fold, actions)
	})
}

// userFold is whether a user message folds: long when its text lays out in
// more lines than a collapsed message shows, open while the user expanded it
// (the disclosure key).
type userFold struct {
	key        string
	long, open bool
}

// userMessage is the muted bubble on the right, in the package's user message
// colors, border and radius, with its corner art; marks, when set, are its
// text with the search's matches marked. Unless expanded, it shows the first
// collapsedLines lines of the text as it wraps at the bubble's width. It
// returns the text's paragraph, the box that shows it and whether the text
// lays out in more lines than a collapsed bubble shows.
func userMessage(c *ui.Context, text string, marks []ui.Span, references []Reference, expanded bool) (body, shown ui.Element, long bool) {
	size := textSize(c)
	// Every line is at least the leading high, and system fonts reach no
	// higher at 1.6 ems: half a line past the collapsed ones is a line more.
	line := size * userLeading
	limit := (collapsedLines + 0.5) * line
	ui.Row(c).Justify(ui.End).Children(func() {
		// The text keeps the bubble's sides as its padding, and the bubble its
		// top and bottom, so the clip ends at a line.
		bubble(c, ui.Column(c.Key("bubble")).Padding(9, 0).MaxWidth(bubbleMax)).Children(func() {
			shown = ui.Column(c.Key("text")).ClipY()
			shown.Children(func() {
				if len(references) > 0 {
					body = referenceText(c, text, marks, references)
				} else if marks != nil {
					body = ui.RichText(c, marks...)
				} else {
					body = ui.Text(c, text)
				}
				body.FontSize(size).LineHeight(userLeading).PaddingX(14).Shrink(0)
			})
		})
	})
	// MyGo v0.10.0 reports neither a text's lines nor that MaxLines cut it,
	// only the box its layout takes. Collapsed, the text lays out one line
	// past those shown, under a clip, so that box tells a longer text without
	// a second layout or laying out the rest of it; expanded, it is all of it.
	// The box is the last frame's: painting a box on the other side of the
	// limit builds again, so the toggle follows a new width or text size.
	long = body.LayoutBounds().H > limit
	if !expanded {
		body.MaxLines(collapsedLines + 1)
		shown.MaxHeight(collapsedLines * line)
	}
	body.Draw(func(_ *ui.Painter, r ui.Rect) {
		if (r.H > limit) != long {
			c.Invalidate()
		}
	})
	return body, shown, long
}

// bubble gives b the user message's look: the theme's colors, border, radius
// and corner art.
func bubble(c *ui.Context, b ui.Element) ui.Element {
	p := theme.Of(c)
	// A step lighter than the muted surface on a dark page, so the message
	// stands apart from the replies around it.
	fill := p.Muted
	if p.Dark {
		fill = fill.Mix(kit.Opaque(p.Foreground), userLift)
	}
	b.Radius(p.RadiusLg * 1.5).Background(kit.Veil(c, fill)).TextColor(p.Foreground)
	v := p.Variant
	if v == nil {
		return b
	}
	if m, ok := v.Messages["user"]; ok {
		b.Background(kit.Veil(c, m.Background.Or(p.Muted))).TextColor(m.Foreground.Or(p.Foreground))
		if m.Radius != nil {
			b.Radius(*m.Radius)
		}
		if m.BorderWidth > 0 {
			b.Border(m.BorderWidth, m.Border.Or(p.Border))
		}
	}
	if v.Asset("message.user.top_left") != nil || v.Asset("message.user.bottom_right") != nil {
		b.Draw(func(pt *ui.Painter, r ui.Rect) { kit.Corners(pt, r, v, "message.user") })
	}
	return b
}

// messageTheme scales the caller-owned Markdown theme without changing Kit's
// shared theme. MyGo invalidates retained text geometry when this value changes.
func (s *State) messageTheme(c *ui.Context) {
	base, size := kit.MarkdownTheme(c, kit.Chat), textSize(c)
	if s.mdBase == base && s.mdSize == size {
		return
	}
	copy := *base
	copy.FontSize = size
	s.md.Theme = &copy
	s.mdBase = base
	s.mdSize = size
}

// StatusKey is the run's status label (frame::status).
func StatusKey(st RunStatus) string {
	switch st {
	case Queued:
		return "turn_queued"
	case Running:
		return "chat_running"
	case Stopping:
		return "chat_stopping"
	case Continued:
		return "turn_continued"
	case Cancelled:
		return "turn_cancelled"
	case Interrupted:
		return "chat_interrupted"
	case Failed:
		return "turn_failed"
	case Unsynced:
		return "chat_status_unsynced"
	}
	return "turn_completed"
}

// Elapsed formats a run's time as the reference does: 9s, 1m05s, 1h02m05s.
func Elapsed(d time.Duration) string {
	s := int(d / time.Second)
	switch {
	case s < 60:
		return fmt.Sprintf("%ds", s)
	case s < 3600:
		return fmt.Sprintf("%dm%02ds", s/60, s%60)
	}
	return fmt.Sprintf("%dh%02dm%02ds", s/3600, s/60%60, s%60)
}

// reply is the assistant's message: a header with the run's status over a
// separator, the work region folded under it, the answer, and the footer once
// the run ends.
func (s *State) reply(c *ui.Context, t *Turn, mark string, actions Actions) {
	s.replyPart(c, t, cited(t.Reply.Blocks, t.Reply.Citations), mark, actions, true, t.ID)
}

func (s *State) replyPart(c *ui.Context, t *Turn, blocks []Block, mark string, actions Actions, latest bool, key string) {
	p := theme.Of(c)
	size := textSize(c)
	r := &t.Reply
	partStatus := r.Status
	if !latest {
		partStatus = Completed
	}
	workKey := "work-" + key
	workOpen, chosen := s.work[workKey]
	if !chosen {
		// Every part's work stays open until the run completes, the parts
		// before consumed guidance included, so the reply the user steered
		// from remains readable; then all of them fold. A run that ended
		// without completing keeps them open to show where it stopped.
		workOpen = r.Status != Completed
	}
	parts := partitionReply(blocks, workOpen)
	active := r.Status.Active()
	live := latest && active && r.Status != Unsynced
	shimmerTools := r.Status == Running || r.Status == Stopping
	gap := float32(8)
	if parts.hasWork {
		gap = workGap
	}
	col := ui.Column(c.Key("reply")).Gap(gap)
	over := col.Hovered()
	col.Children(func() {
		// The header: the status, its time, and the work region's trigger.
		// Every part keeps it, so a reply answered before consumed guidance
		// still reads as its own reply and its work can be folded.
		{
			ui.Column(c).Gap(8).Children(func() {
				label := i18n.T(c, StatusKey(partStatus))
				status := func() {
					row := ui.Row(c).Gap(8).Shrink(1).MinWidth(0)
					if latest && (r.Status == Failed || r.Status == Interrupted) {
						row.TextColor(p.Danger)
					}
					row.Children(func() {
						if live && r.Phase == ExecutingTool && !parts.runningToolVisible() {
							kit.Shimmer(c, label, size, p.MutedForeground)
						} else {
							ui.Text(c, label).FontSize(size).SingleLine()
						}
						if latest && r.Elapsed > 0 {
							ui.Text(c, Elapsed(r.Elapsed)).FontSize(size).SingleLine()
						}
					})
				}
				// A quiet line: the status in a lighter tone than the answer,
				// the work region's chevron right after it, brightening on
				// hover.
				if !parts.hasWork {
					ui.Row(c).Height(rowHeight(size)).TextColor(p.Description()).Children(status)
				} else {
					b := ui.ButtonBase(c.Key("work")).Label(label).Height(rowHeight(size)).MaxWidthPercent(100).Gap(6).
						Justify(ui.Start).Expanded(workOpen).Cursor(ui.CursorPointer).TextColor(p.Description())
					if b.Hovered() || b.FocusVisible() {
						b.TextColor(p.MutedForeground)
					}
					session := s.renderSession
					b.Children(func() {
						status()
						chevron := icons.ChevronRight
						if workOpen {
							chevron = icons.ChevronDown
						}
						ui.Icon(c, chevron).FontSize(rowIcon(size) - 2).Shrink(0)
					}).OnClick(func() { s.setWork(session, workKey, !workOpen) })
				}
				ui.Divider(c).Background(p.Border)
				if parts.hasWork && workOpen {
					ui.Column(c.Key("work-body")).Gap(workGap).Children(func() {
						s.blocks(c, workKey, parts.work, live && len(parts.answer) == 0 && len(parts.failures) == 0, shimmerTools, r.Phase, actions)
					})
				}
			})
		}
		if len(parts.answer) > 0 {
			answer := parts.answer
			if mark != "" {
				answer = markBlocks(answer, mark)
			}
			ui.Column(c.Key("answer")).Gap(workGap).Children(func() {
				s.blocks(c, key, answer, live, shimmerTools, r.Phase, actions)
			})
		}
		if latest && !active && r.Changes != nil {
			s.changes(c, t.ID, r.Changes, actions)
		}
		if live && showPhase(r, parts) {
			phaseRow(c, r, t.ID, actions)
		} else if live {
			// The phase row comes and goes with every call, question and
			// streamed block; its place stays for the whole run so a
			// transcript resting on its end moves only when content grows.
			ui.Box(c.Key("phase")).Height(rowHeight(textSize(c)))
		}
		if len(parts.failures) > 0 {
			ui.Column(c.Key("errors")).Gap(workGap).Children(func() {
				s.blocks(c, key+"-errors", parts.failures, false, false, PhaseNone, actions)
			})
		}
		if latest && !active {
			s.footer(c, t, over, actions)
		}
	})
}

// MyGo reshapes an open Markdown paragraph for each distinct source. Batch
// rapid appends to longer replies without delaying short, slow or final text.
const streamPreviewInterval = 80 * time.Millisecond

type streamPreview struct {
	shown string
	next  time.Time
}

func streamSource(c *ui.Context, row ui.Element, src string, streaming bool) string {
	p := ui.Local(row, "stream-preview", func() streamPreview { return streamPreview{} })
	if !streaming || len(src) < 1024 {
		*p = streamPreview{}
		return src
	}
	if src != p.shown {
		now := c.Now()
		if now.Before(p.next) && strings.HasPrefix(src, p.shown) {
			c.After(p.next.Sub(now))
			return p.shown
		}
		p.shown, p.next = src, now.Add(streamPreviewInterval)
	}
	return src
}

// blocks builds blocks in transcript order.
func (s *State) blocks(c *ui.Context, turn string, blocks []Block, continuing, shimmerTools bool, phase Phase, actions Actions) {
	p := theme.Of(c)
	size := textSize(c)
	for i, b := range blocks {
		key := fmt.Sprintf("%s-%d", turn, i)
		last := i == len(blocks)-1
		row := ui.Column(c.Key(key))
		row.Children(func() {
			switch b.Kind {
			case TextBlock:
				streaming := continuing && last && phase == Generating
				text := streamSource(c, row, b.Text, streaming)
				s.span(kit.Markdown(c, text, kit.MarkdownProps{Variant: kit.Chat, Animated: streaming, Options: &s.md}), 0, 0, 0)
			case ReasoningBlock:
				head := Call{Label: i18n.T(c, "turn_reasoning"), Icon: "bot", Running: continuing && last && phase == Thinking}
				s.disclosure(c, "reasoning-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
					ui.Scroll(c).MaxHeight(160).Children(func() {
						ui.Text(c, b.Text).FontSize(size).LineHeight(22.0 / 14).TextColor(p.MutedForeground)
					})
				})
			case ToolsBlock:
				s.tools(c, b.Calls, shimmerTools)
			case QuestionBlock:
				s.question(c, b.Question)
			case ChildrenBlock:
				s.children(c, b.Children, actions)
			case ProgressBlock:
				s.steps(c, key, b.Steps, continuing)
			case ImagesBlock:
				s.thumbnails(c, "images", b.Images, false)
			case FilesBlock:
				s.files(c, "files", b.Files, actions)
			case SearchBlock:
				s.grounding(c, key, b)
			case RetryBlock:
				s.retry(c, key, b.Retry, continuing && last)
			case VisualBlock:
				visual(c, "visual", b.Visual)
			case CompactionBlock:
				s.compaction(c, &Turn{ID: turn + "-" + b.Label, Compaction: &Compaction{State: Compacted, Summary: b.Text}})
			case LimitBlock:
				// Label names another runtime limit; none is the output limit.
				label := "chat_output_limit"
				if b.Label != "" {
					label = b.Label
				}
				plain(c, "limit-"+key, Call{Label: i18n.T(c, label), Icon: "triangle-alert", Warning: true})
			case ErrorBlock:
				s.diagnostic(c, &Diagnostic{Title: b.Label, Fields: b.Fields, Detail: b.Text})
			}
		})
	}
}

// The tail reports the parent's current phase, not the last activity in its
// history. A background child has its own status independent of the parent.
func showPhase(r *Reply, parts replySections) bool {
	if r.Status == Stopping || r.Phase == WaitingUser || r.Phase == ExecutingTool {
		return false
	}
	if r.Phase == WaitingController {
		return true
	}
	// The model's own progress shows only until the reply's first output:
	// between later steps the row would flash for a moment at each one.
	// Compaction and waiting on children remain distinct states.
	if (r.Phase == AwaitingResponse || r.Phase == Thinking || r.Phase == Generating || r.Phase == PhaseNone) && r.Status == Running &&
		(parts.hasWork || len(parts.answer)+len(parts.failures) > 0) {
		return false
	}
	if len(parts.work) > 0 && parts.endsInRetry && (r.Phase == AwaitingResponse || r.Phase == PhaseNone) && r.Status == Running {
		return false
	}
	// A committed reply or dismissed question remains visible after its draft
	// disappears and before the run's next status arrives.
	if r.Status == Running && (r.Phase == AwaitingResponse || r.Phase == PhaseNone || r.Phase == Thinking) && len(parts.answer) > 0 {
		last := parts.answer[len(parts.answer)-1]
		switch last.Kind {
		case TextBlock, ImagesBlock, FilesBlock, VisualBlock, LimitBlock:
			return false
		case QuestionBlock:
			if q := last.Question; q != nil && q.State != QuestionAnswered && q.State != QuestionAccepted {
				return false
			}
		}
	}
	visibleTail := parts.answer
	if len(visibleTail) == 0 && len(parts.failures) == 0 {
		visibleTail = parts.work
	}
	if n := len(visibleTail); n > 0 {
		last := visibleTail[n-1]
		if (last.Kind == TextBlock && r.Phase == Generating || last.Kind == ReasoningBlock && r.Phase == Thinking) && last.Text != "" {
			return false
		}
		// The latest call's own sweep already shows the model being awaited;
		// the task's progress card below it does not count as later output.
		for i := n - 1; i >= 0; i-- {
			if b := visibleTail[i]; b.Kind != ProgressBlock {
				if b.Kind == ToolsBlock && len(b.Calls) > 0 && awaitingModel(b.Calls[len(b.Calls)-1]) {
					return false
				}
				break
			}
		}
	}
	for _, blocks := range [][]Block{parts.work, parts.answer} {
		for _, b := range blocks {
			for _, call := range b.Calls {
				if call.Pending || call.Running {
					return false
				}
			}
			if b.Question != nil && b.Question.State == QuestionPending {
				return false
			}
		}
	}
	return true
}

// phaseRow is the active run's last row: the phase's mark (kit.Indicator)
// and what the run does.
func phaseRow(c *ui.Context, r *Reply, turn string, actions Actions) {
	p := theme.Of(c)
	var label string
	switch r.Phase {
	case Generating:
		label = i18n.T(c, "turn_generating")
	case AwaitingResponse:
		label = i18n.T(c, "turn_awaiting_response")
	case Compacting:
		label = i18n.T(c, "turn_compacting")
		if r.PhaseElapsed >= time.Second {
			label += " " + Elapsed(r.PhaseElapsed)
		}
	case WaitingChildren:
		if r.Waiting == 1 {
			label = i18n.T(c, "turn_waiting_child")
		} else {
			label = i18n.T(c, "turn_waiting_children", "count", r.Waiting)
		}
	case WaitingController:
		label = i18n.T(c, "turn_waiting_controller")
	default:
		label = i18n.T(c, "turn_thinking")
	}
	mark := Indicator(r.Phase)
	// A run queued behind another has no phase yet: it waits.
	if r.Status == Queued {
		label, mark = i18n.T(c, "turn_queued"), kit.Waiting
	}
	// One band across the row, past the indicator.
	sw := &kit.Sweep{}
	size := textSize(c)
	row := ui.Row(c.Key("phase")).Gap(8).Height(rowHeight(size))
	row.Children(func() {
		kit.Indicator(c, mark, rowIcon(size))
		// Laid out as the sweep draws it (kit.Sweep.Text): a RichText.
		span := ui.Span{Text: label, Size: size, Color: p.MutedForeground}
		sw.Text(c, levelSpan(c, ui.RichText(c, span).SingleLine(), span, size, false), span)
		if r.Phase == WaitingController && r.LoginRecovery {
			kit.Secondary(c.Key("resume-login"), i18n.T(c, "controller_login_restore")).
				OnClick(func() { actions.ResumeLogin(turn) })
		}
	})
	sw.Frame(row)
}

// Indicator is the thinking indicator's mark for a phase (ui/thinking
// state): searching while it thinks or waits, composing while it writes,
// working while children run, the still mark otherwise.
func Indicator(ph Phase) kit.Thinking {
	switch ph {
	case Thinking, AwaitingResponse, PhaseNone:
		return kit.Searching
	case Generating:
		return kit.Composing
	case WaitingChildren:
		return kit.Working
	}
	return kit.Breathing
}
