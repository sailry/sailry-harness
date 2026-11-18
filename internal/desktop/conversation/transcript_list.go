package conversation

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown/parser"
)

// MyGo retains Key and the last row builder on ListState. Both refer to this
// list, which can drop its frame data without losing MyGo's scroll anchor.
type transcriptList struct {
	layout  ui.ListState
	row     func(int)
	link    func(string)
	block   func(*ui.Context, *parser.Document, int) bool
	nav     *navigation
	frame   transcriptFrame
	mounted bool
}

type transcriptFrame struct {
	state   *State
	c       *ui.Context
	id      string
	turns   []Turn
	session *Session
	actions Actions
	aside   float32
}

// List returns the list state of a conversation.
func (s *State) List(id string) *ui.ListState { return &s.listOf(id).layout }

func (s *State) listOf(id string) *transcriptList {
	if l := s.lists[id]; l != nil {
		return l
	}
	l := &transcriptList{}
	l.layout.FollowEnd, l.layout.Key = true, l.key
	l.row, l.link, l.block = l.buildRow, l.openLink, l.embedBlock
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
	return l
}

// FinishFrame drops the render bindings of lists and rails not built in this
// view pass. Call it once after building all panes and overlays. Mounted lists
// keep their bindings because MyGo can build more rows during layout.
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
			l.frame = transcriptFrame{}
		}
	}
	clear(s.boundLists[len(bound):])
	s.boundLists = bound
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
	session, block, link := s.renderSession, s.md.Block, s.md.OnLink
	s.renderSession, s.md.Block, s.md.OnLink = f.id, nil, l.link
	if s.Embed != nil {
		s.md.Block = l.block
	}
	defer func() { s.renderSession, s.md.Block, s.md.OnLink = session, block, link }()
	// Row actions can keep a turn, but must not keep the whole slice alive
	// through a pointer into its backing array.
	t := f.turns[i]
	s.turn(f.c, f.id, i, &t, len(f.turns), f.session, f.actions)
}

func (l *transcriptList) openLink(link string) {
	f := &l.frame
	if f.state != nil {
		f.state.follow(f.c, f.id, link)
	}
}

func (l *transcriptList) embedBlock(c *ui.Context, d *parser.Document, i int) bool {
	f := &l.frame
	return f.state != nil && f.state.Embed != nil && f.state.embed(c, f.id, d, i, f.actions)
}
