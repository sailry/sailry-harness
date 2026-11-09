package conversation

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
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
	input   ui.Handle
	focus   bool // focus the input in the next build
}

// OpenSearch opens the search bar on conversation id, or focuses it.
func (s *State) OpenSearch(id string) {
	if s.find.conv != id {
		s.find = search{conv: id}
	}
	s.find.open, s.find.focus = true, true
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

// step moves to the next or previous match, wrapping, and reveals it.
func (s *State) step(id string, next bool) {
	f := &s.find
	if len(f.matches) == 0 {
		return
	}
	if next {
		f.index = (f.index + 1) % len(f.matches)
	} else {
		f.index = (f.index - 1 + len(f.matches)) % len(f.matches)
	}
	s.List(id).ScrollTo(f.matches[f.index].Turn, ui.Center)
}

// searchBar is the bar: the query, its count, previous, next and close,
// on the floating color, over the transcript's top end.
func (s *State) searchBar(c *ui.Context, id string, turns []Turn) {
	f := &s.find
	if !f.open || f.conv != id {
		return
	}
	p := theme.Of(c)
	if f.query != f.shown {
		f.shown, f.index = f.query, 0
		f.matches = Find(turns, f.query)
		if len(f.matches) > 0 {
			s.List(id).ScrollTo(f.matches[0].Turn, ui.Center)
		}
	}
	w, _ := c.Size()
	bar := ui.Row(c.Key("search-bar")).Absolute().Top(8).Right(16).Width(min(360, w-32)).Height(40).Padding(4, 4, 4, 12).Gap(4).
		Radius(p.RadiusLg).Background(p.Floating()).Border(1, p.Border).Shadow(0, 6, 20, 0, p.Backdrop().Alpha(0.18)).
		AlignItems(ui.Center).Label(i18n.T(c, "chat_search"))
	if bar.OverlayShortcut(0, ui.KeyEscape) || bar.Shortcut(0, ui.KeyEscape) {
		f.open = false
		return
	}
	bar.Children(func() {
		ui.Icon(c, icons.Search).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
		in := ui.TextInputBase(c.Key("query"), &f.query).Placeholder(i18n.T(c, "chat_search_query")).PlaceholderColor(p.MutedForeground).
			Label(i18n.T(c, "chat_search_query")).FontSize(14).Grow(1).MinWidth(0).MaxLength(searchLimit).Bind(&f.input)
		in.OnTextKey(func(k ui.TextKey) bool {
			switch {
			case k.Key == ui.KeyEnter && k.Mods == 0, k.Key == ui.KeyDown && k.Mods == 0:
				s.step(id, true)
			case k.Key == ui.KeyEnter && k.Mods == ui.Shift, k.Key == ui.KeyUp && k.Mods == 0:
				s.step(id, false)
			default:
				return false
			}
			return true
		})
		if f.focus {
			f.focus = false
			f.input.Focus(c)
		}
		count := ""
		switch {
		case len(f.matches) > 0:
			count = fmt.Sprintf("%d/%d", f.index+1, len(f.matches))
		case strings.TrimSpace(f.query) != "":
			count = "0/0"
		}
		ui.Text(c, count).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
		none := len(f.matches) == 0
		kit.IconButton(c.Key("prev"), icons.ChevronUp, i18n.T(c, "chat_image_previous"), 28, false).Disabled(none).OnClick(func() { s.step(id, false) })
		kit.IconButton(c.Key("next"), icons.ChevronDown, i18n.T(c, "chat_image_next"), 28, false).Disabled(none).OnClick(func() { s.step(id, true) })
		kit.IconButton(c.Key("close"), icons.Close, i18n.T(c, "close"), 28, false).OnClick(func() { f.open = false })
	})
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
