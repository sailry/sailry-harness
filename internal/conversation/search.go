package conversation

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/theme"
)

// Search in the conversation (live/search.rs, search/results.rs,
// search/reveal.rs): ⌘F opens a bar over the transcript's top end; the
// query matches, ignoring case, the user messages and the answers; the
// matches are marked in the turns shown, the current one revealed, Enter
// and the arrows step through them, Escape closes the bar.

// searchLimit is the longest query (chat_search_limit).
const searchLimit = 200

// Match is an occurrence of the query: its turn, whether in the user
// message, and which occurrence of that text it is.
type Match struct {
	Turn int
	User bool
	N    int
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
// code spans and fenced code, keeping the text's own case.
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
			b.WriteString(markPlain(part, q, "<mark>", "</mark>"))
		}
	}
	return b.String()
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

// search is the bar's state for the conversation it searches.
type search struct {
	open    bool
	conv    string
	query   string
	shown   string // the query the matches are of
	matches []Match
	index   int
}

// OpenSearch opens the search bar on conversation id, or focuses it.
func (s *State) OpenSearch(id string) {
	if s.find.conv != id {
		s.find = search{conv: id}
	}
	s.find.open = true
}

// SearchOpen reports whether the search bar shows.
func (s *State) SearchOpen() bool { return s.find.open }

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
	f := &s.find
	if !f.open || f.conv != id {
		return
	}
	if r := []rune(f.query); len(r) > searchLimit {
		f.query = string(r[:searchLimit])
	}
	if f.query != f.shown {
		f.shown, f.index = f.query, 0
		f.matches = Find(turns, f.query)
		if len(f.matches) > 0 {
			s.List(id).ScrollTo(f.matches[0].Turn, ui.Center)
		}
	}
	w, _ := c.Size()
	bar := kit.Floating(c, ui.FindBar(c.Key("search-bar"), &f.open, &f.query, len(f.matches), &f.index)).
		Absolute().Top(8).Right(16).Width(min(420, w-32))
	if bar.Changed() && len(f.matches) > 0 {
		s.List(id).ScrollTo(f.matches[f.index].Turn, ui.Center)
	}
	if bar.OverlayShortcut(0, ui.KeyEscape) {
		f.open = false
	}
}

// highlight returns what turn i shows with the search's marks: its user
// message's spans and its answer marked, or the turn unchanged.
func (s *State) highlight(i int, t *Turn) (user string, current int, marked bool) {
	f := &s.find
	if !f.open || f.shown == "" || len(f.matches) == 0 {
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
