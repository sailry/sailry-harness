package conversation

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown/parser"
)

// MyGo retains its last row frame in ListState. Keep it only while mounted.
type transcriptList struct {
	layout       ui.ListState
	scroll       ui.ScrollState
	olderMarker  ui.Handle
	olderTurn    string
	olderVisible bool
	olderRetry   bool
	row          func(int)
	link         func(string)
	block        func(*ui.Context, *parser.Document, int) bool
	paint        func(*ui.Painter, ui.Rect)
	nav          *navigation
	frame        transcriptFrame
	paintTop     float32
	painted      transcriptAnchor
	restore      transcriptAnchor
	mounted      bool
}

type transcriptAnchor struct {
	turn  string
	inset float32
	order uint64
}

const transcriptBookmarks = 64

type transcriptFrame struct {
	state   *State
	c       *ui.Context
	id      string
	turns   []Turn
	session *Session
	actions Actions
	aside   float32
}

// List returns a mounted layout, or prepares one for an explicit scroll
// request before the conversation is shown.
func (s *State) List(id string) *ui.ListState {
	l := s.listOf(id)
	delete(s.anchors, id)
	l.restore = transcriptAnchor{}
	return &l.layout
}

// HasTranscriptAnchor reports whether conversation id has a reading position
// saved when its transcript was last unmounted away from its end, which its
// next mount restores.
func (s *State) HasTranscriptAnchor(id string) bool {
	_, ok := s.anchors[id]
	return ok
}

func (s *State) listOf(id string) *transcriptList {
	if l := s.lists[id]; l != nil {
		return l
	}
	l := &transcriptList{}
	l.layout.FollowEnd, l.layout.Key = true, l.key
	l.row, l.link, l.block, l.paint = l.buildRow, l.openLink, l.embedBlock, l.beginPaint
	s.lists[id] = l
	return l
}

func (s *State) bindTranscript(c *ui.Context, id string, turns []Turn, sess *Session, actions Actions) *transcriptList {
	l := s.listOf(id)
	if l.frame.state == nil {
		s.boundLists = append(s.boundLists, l)
	}
	l.frame = transcriptFrame{state: s, c: c, id: id, turns: turns, session: sess, actions: actions, aside: s.Aside}
	l.mounted = true
	s.restoreTranscript(l)
	return l
}

// FinishFrame releases lists and rails left out of this pass. Call it after
// building every pane and overlay. Mounted lists can build more rows in layout.
func (s *State) FinishFrame() {
	s.navigated = nil
	bound := s.boundLists[:0]
	for _, l := range s.boundLists {
		if u := l.nav; u != nil {
			if !u.mounted {
				u.c, u.transcript = nil, nil
			}
			u.mounted = false
		}
		if l.mounted {
			l.mounted = false
			bound = append(bound, l)
		} else {
			s.releaseTranscript(l)
			l.frame = transcriptFrame{}
			l.olderMarker, l.olderTurn = ui.Handle{}, ""
			l.olderVisible, l.olderRetry = false, false
			l.layout, l.scroll = ui.ListState{}, ui.ScrollState{}
		}
	}
	clear(s.boundLists[len(bound):])
	s.boundLists = bound
}

func (s *State) releaseTranscript(l *transcriptList) {
	id := l.frame.id
	first, last := l.layout.Visible()
	if !l.layout.AtEnd() && first >= 0 && first <= last && first < len(l.frame.turns) {
		delete(s.anchors, id)
		turn := l.frame.turns[first].ID
		place := transcriptAnchor{turn: turn}
		if l.painted.turn == turn {
			place.inset = l.painted.inset
		}
		s.rememberTranscript(id, place)
	} else if sess := l.frame.session; sess == nil || sess.OlderFailed || sess.Older == 0 && !sess.Loading && !sess.LoadingOlder {
		// If the loaded range can still grow to reach a pending bookmark,
		// keep it when leaving the transcript during history recovery.
		delete(s.anchors, id)
	}
	delete(s.lists, id)
	delete(s.docks, id)
	delete(s.views, id)
	delete(s.presses, id)
	delete(s.area, id)
	delete(s.built, id)
	s.CloseSearchIn(id)
	delete(s.finds, id)
}

func (s *State) rememberTranscript(id string, place transcriptAnchor) {
	if s.anchors == nil {
		s.anchors = make(map[string]transcriptAnchor)
	}
	if len(s.anchors) >= transcriptBookmarks {
		var oldest string
		var order uint64
		for session, anchor := range s.anchors {
			if order == 0 || anchor.order < order {
				oldest, order = session, anchor.order
			}
		}
		delete(s.anchors, oldest)
	}
	s.anchorOrder++
	place.order = s.anchorOrder
	s.anchors[id] = place
}

func (s *State) restoreTranscript(l *transcriptList) {
	f := &l.frame
	place, ok := s.anchors[f.id]
	if !ok {
		return
	}
	// A new reading position or a failed history load ends a restore attempt.
	if s.built[f.id] && !l.layout.AtEnd() || f.session != nil && f.session.OlderFailed {
		delete(s.anchors, f.id)
		return
	}
	if len(f.turns) == 0 {
		return
	}
	for row := range f.turns {
		if f.turns[row].ID == place.turn {
			l.layout.ScrollTo(row, ui.Start)
			l.restore = place
			delete(s.anchors, f.id)
			return
		}
	}
	// A reopening can render its newest page while Client restores older turns.
	if f.session == nil || f.session.Older == 0 {
		delete(s.anchors, f.id)
	}
}

func (l *transcriptList) beginPaint(_ *ui.Painter, rect ui.Rect) {
	l.paintTop = rect.Y
}

func (l *transcriptList) paintRow(turn string, rect ui.Rect) {
	// MyGo exposes scrolling in pixels and scrolling to a row separately.
	// Measure the row's inset here, after the virtual list has laid it out.
	inset := l.paintTop - rect.Y
	if inset >= -0.5 && inset < rect.H {
		l.painted = transcriptAnchor{turn: turn, inset: max(0, inset)}
	}
	if l.restore.turn == turn {
		adjust := min(l.restore.inset, max(0, rect.H-1)) - max(0, inset)
		l.restore = transcriptAnchor{}
		if adjust > 0.5 && l.scroll.Y < l.scroll.MaxY {
			l.scroll.Y = min(l.scroll.Y+adjust, l.scroll.MaxY)
			l.frame.c.Invalidate()
		}
	}
}

func (l *transcriptList) key(i int) any {
	if i >= 0 && i < len(l.frame.turns) {
		return l.frame.turns[i].ID
	}
	return i
}

func (l *transcriptList) buildRow(i int) {
	f := &l.frame
	if i < 0 || i >= len(f.turns) {
		return
	}
	s := f.state
	resolveImage := s.resolveImage
	s.resolveImage = nil
	if f.session != nil {
		s.resolveImage = f.session.ResolveImage
	}
	session, block, link := s.renderSession, s.md.Block, s.md.OnLink
	s.renderSession, s.md.Block, s.md.OnLink = f.id, nil, l.link
	if s.Embed != nil || f.session != nil && f.session.Embed != nil {
		s.md.Block = l.block
	}
	defer func() {
		s.renderSession, s.md.Block, s.md.OnLink, s.resolveImage = session, block, link, resolveImage
	}()
	// Row actions can keep a turn, but must not keep the whole slice alive
	// through a pointer into its backing array.
	t := f.turns[i]
	turn := t.ID
	s.turn(f.c, f.id, i, &t, len(f.turns), f.session, f.actions).DrawOver(func(_ *ui.Painter, rect ui.Rect) {
		l.paintRow(turn, rect)
	})
}

func (l *transcriptList) openLink(link string) {
	f := &l.frame
	if f.state != nil {
		f.state.follow(f.c, f.id, link)
	}
}

func (l *transcriptList) embedBlock(c *ui.Context, d *parser.Document, i int) bool {
	f := &l.frame
	if f.state == nil {
		return false
	}
	resolve := f.state.Embed
	if f.session != nil && f.session.Embed != nil {
		resolve = f.session.Embed
	}
	return resolve != nil && f.state.embed(c, f.id, d, i, f.actions, resolve)
}
