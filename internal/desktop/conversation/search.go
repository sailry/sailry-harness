package conversation

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Search in the conversation: ⌘F opens a bar over the transcript's top end;
// the query matches, ignoring case, the user messages and the answers; the
// matches are marked in the turns shown, the current one revealed, Enter and
// the arrows step through them, Escape closes the bar.

// searchLimit is the longest query (chat_search_limit).
const searchLimit = 200

// Match is an occurrence of the query: its turn, whether in the user
// message, and which occurrence of that text it is.
type Match struct {
	Turn                             int
	User                             bool
	N                                int
	Target, Entry                    string
	Sequence, TurnSequence, Revision uint64
}

type SearchPage struct {
	Matches    []Match
	NextBefore *uint64
}

// Find returns every occurrence of query in the turns, in order: each
// turn's user message, then its answer. An empty query finds none.
func Find(turns []Turn, query string) []Match {
	q := strings.ToLower(strings.TrimSpace(query))
	if q == "" {
		return nil
	}
	var out []Match
	for i := range turns {
		for n := range strings.Count(strings.ToLower(turns[i].User), q) {
			out = append(out, Match{Turn: i, User: true, N: n})
		}
		for n := range strings.Count(strings.ToLower(searchable(turns[i].Reply.Blocks)), q) {
			out = append(out, Match{Turn: i, N: n})
		}
	}
	return out
}

// searchable is the answer text search looks in.
func searchable(blocks []Block) string {
	return AnswerText(Compact(blocks))
}

// Mark wraps each occurrence of query in Markdown src with <mark>, outside
// code spans, fenced code and link destinations, keeping the text's own
// case.
func Mark(src, query string) string {
	q := strings.TrimSpace(query)
	if q == "" {
		return src
	}
	var b strings.Builder
	fenced := false
	for _, line := range strings.SplitAfter(src, "\n") {
		trim := strings.TrimLeft(line, " ")
		if strings.HasPrefix(trim, "```") || strings.HasPrefix(trim, "~~~") {
			fenced = !fenced
			b.WriteString(line)
			continue
		}
		if fenced {
			b.WriteString(line)
			continue
		}
		// Outside inline code, between backticks.
		parts := strings.Split(line, "`")
		for j, part := range parts {
			if j > 0 {
				b.WriteByte('`')
			}
			if j%2 == 1 && j < len(parts)-1 {
				b.WriteString(part)
				continue
			}
			markProse(&b, part, q)
		}
	}
	return b.String()
}

// markProse marks q in s but for the destinations of its links, "](" to the
// ")" closing it, which stay the addresses they are: a citation's, as Cite
// writes it, or the author's.
func markProse(b *strings.Builder, s, q string) {
	for {
		i := strings.Index(s, "](")
		if i < 0 {
			break
		}
		end := max(destinationEnd(s[i+2:]), 0) // not a link: text after all
		b.WriteString(markPlain(s[:i], q, "<mark>", "</mark>"))
		b.WriteString(s[i : i+2+end])
		s = s[i+2+end:]
	}
	b.WriteString(markPlain(s, q, "<mark>", "</mark>"))
}

// destinationEnd is the length of a link's destination and the ")" closing
// it at the start of s, -1 when the line ends before it closes: the pointed
// form ends at ">)", the plain one where its parentheses balance.
func destinationEnd(s string) int {
	if strings.HasPrefix(s, "<") {
		if i := strings.Index(s, ">)"); i >= 0 {
			return i + 2
		}
		return -1
	}
	depth := 0
	for i := 0; i < len(s); i++ {
		switch s[i] {
		case '(':
			depth++
		case ')':
			if depth == 0 {
				return i + 1
			}
			depth--
		}
	}
	return -1
}

// markPlain wraps the occurrences of q in s, ignoring case.
func markPlain(s, q, open, close string) string {
	ls, lq := strings.ToLower(s), strings.ToLower(q)
	if len(ls) != len(s) || !strings.Contains(ls, lq) {
		// Case folding changed the length: leave it unmarked rather than cut
		// a rune.
		return s
	}
	var b strings.Builder
	at := 0
	for {
		i := strings.Index(ls[at:], lq)
		if i < 0 {
			break
		}
		b.WriteString(s[at : at+i])
		b.WriteString(open)
		b.WriteString(s[at+i : at+i+len(q)])
		b.WriteString(close)
		at += i + len(q)
	}
	b.WriteString(s[at:])
	return b.String()
}

// spans splits plain text at the occurrences of q, for the user bubble:
// the current one stands out.
func spans(text, q string, current int, p *theme.Palette) []ui.Span {
	ls, lq := strings.ToLower(text), strings.ToLower(strings.TrimSpace(q))
	if lq == "" || len(ls) != len(text) {
		return []ui.Span{{Text: text}}
	}
	var out []ui.Span
	at, n := 0, 0
	for {
		i := strings.Index(ls[at:], lq)
		if i < 0 {
			break
		}
		out = append(out, ui.Span{Text: text[at : at+i]})
		bg := p.Warning.Alpha(0.3)
		if n == current {
			bg = p.Warning.Alpha(0.6)
		}
		out = append(out, ui.Span{Text: text[at+i : at+i+len(lq)], Background: bg, Radius: 2})
		at += i + len(lq)
		n++
	}
	return append(out, ui.Span{Text: text[at:]})
}

// search is a conversation's search bar: the state is kept by conversation,
// closed or open, so its query is there when the bar opens again.
type search struct {
	open           bool
	query          string
	shown          string // the query the matches are of
	matches        []Match
	index          int
	before         *uint64
	loading        bool
	cancel, reveal func()
	generation     uint64
	selected       int
	revision       uint64
}

// OpenSearch opens the search bar on conversation id, or focuses it. Each
// conversation has its own bar: the panes of a split search side by side,
// and opening one leaves the others as they are.
func (s *State) OpenSearch(id string) {
	f := s.finds[id]
	if f == nil {
		if s.finds == nil {
			s.finds = map[string]*search{}
		}
		f = &search{selected: -1}
		s.finds[id] = f
	}
	if !f.open && s.Search != nil {
		f.shown = ""
	}
	f.open = true
}

func (f *search) stop() {
	if f.cancel != nil {
		f.cancel()
		f.cancel = nil
	}
	if f.reveal != nil {
		f.reveal()
		f.reveal = nil
	}
}

// SearchOpen reports whether a conversation's search bar is open.
func (s *State) SearchOpen() bool { return len(s.SearchSessions()) > 0 }

// SearchOpenIn reports whether conversation id's search bar is open.
func (s *State) SearchOpenIn(id string) bool {
	f := s.finds[id]
	return f != nil && f.open
}
func (s *State) SyncSearch(id string, revision uint64) {
	f := s.finds[id]
	if f != nil && f.open && f.revision != revision {
		f.stop()
		f.generation++
		f.revision = revision
		f.shown = ""
	}
}

// SearchSessions are the conversations whose search bar is open, in order.
func (s *State) SearchSessions() []string {
	var open []string
	for id, f := range s.finds {
		if f.open {
			open = append(open, id)
		}
	}
	slices.Sort(open)
	return open
}

// SearchSession is the first of the conversations whose search bar is open.
func (s *State) SearchSession() string {
	if open := s.SearchSessions(); len(open) > 0 {
		return open[0]
	}
	return ""
}

// CloseSearch closes every search bar.
func (s *State) CloseSearch() {
	for id := range s.finds {
		s.CloseSearchIn(id)
	}
}

// CloseSearchIn closes conversation id's search bar.
func (s *State) CloseSearchIn(id string) {
	if f := s.finds[id]; f != nil {
		f.open = false
		f.stop()
	}
}

// current is the match revealed, if any.
func (f *search) current() (Match, bool) {
	if !f.open || len(f.matches) == 0 {
		return Match{}, false
	}
	return f.matches[f.index], true
}

// searchBar is MyGo's FindBar over the transcript's top end, on the
// floating color: its field, the count, previous, next and Done, with
// Enter, Shift+Enter, Cmd+G and Escape. The matches are this package's
// (Find); a new query reveals the first, and stepping the one chosen.
func (s *State) searchBar(c *ui.Context, id string, turns []Turn) {
	f := s.finds[id]
	if f == nil {
		return
	}
	if !f.open {
		f.stop()
		return
	}
	if r := []rune(f.query); len(r) > searchLimit {
		f.query = string(r[:searchLimit])
	}
	if f.query != f.shown {
		f.stop()
		f.generation++
		f.before = nil
		f.loading = false
		f.selected = -1
		f.shown, f.index = f.query, 0
		if s.Search != nil {
			f.matches = nil
			s.searchPage(id)
		} else {
			f.matches = Find(turns, f.query)
		}
		if len(f.matches) > 0 {
			if s.Search == nil {
				s.List(id).ScrollTo(f.matches[0].Turn, ui.Center)
			}
		}
	}
	w, _ := c.Size()
	bar := kit.Floating(c, ui.FindBar(c.Key("search-bar"), &f.open, &f.query, len(f.matches), &f.index)).
		Absolute().Top(8).Right(16).Width(min(420, w-32))
	if bar.Changed() && len(f.matches) > 0 {
		if s.Search == nil {
			s.List(id).ScrollTo(f.matches[f.index].Turn, ui.Center)
		}
	}
	if s.Search != nil && len(f.matches) > 0 {
		f.index = min(max(f.index, 0), len(f.matches)-1)
		if f.selected != f.index {
			f.selected = f.index
			if f.reveal != nil {
				f.reveal()
			}
			selected, generation := f.index, f.generation
			target := f.matches[selected]
			if s.Reveal != nil {
				f.reveal = s.Reveal(id, target, func(index int, ok bool) {
					if !ok || !f.open || f.generation != generation || f.index != selected {
						return
					}
					f.matches[selected].Turn = index
					s.List(id).ScrollTo(index, ui.Center)
				})
			}
		}
		if f.index == len(f.matches)-1 && f.before != nil && !f.loading {
			s.searchPage(id)
		}
	}
	if bar.OverlayShortcut(0, ui.KeyEscape) {
		f.open = false
	}
	if !f.open {
		f.stop()
	}
}

func (s *State) searchPage(id string) {
	f := s.finds[id]
	if f == nil || s.Search == nil || strings.TrimSpace(f.shown) == "" {
		return
	}
	generation := f.generation
	f.loading = true
	f.cancel = s.Search(id, f.shown, f.before, func(page SearchPage) {
		if f.generation != generation || !f.open {
			return
		}
		f.loading = false
		f.matches = append(f.matches, page.Matches...)
		f.before = page.NextBefore
	})
}

// highlight returns what turn i of conversation id shows with its search's
// marks: its user message's spans and its answer marked, or the turn
// unchanged.
func (s *State) highlight(id string, i int, t *Turn) (user string, current int, marked bool) {
	f := s.finds[id]
	if f == nil || !f.open || f.shown == "" || len(f.matches) == 0 {
		return "", -1, false
	}
	m, _ := f.current()
	current = -1
	if m.Turn == i && m.User {
		current = m.N
	}
	return f.shown, current, true
}

// markBlocks returns a copy of the answer blocks with the query marked.
func markBlocks(blocks []Block, q string) []Block {
	out := append([]Block(nil), blocks...)
	for i := range out {
		if out[i].Kind == TextBlock {
			out[i].Text = Mark(out[i].Text, q)
		}
	}
	return out
}
