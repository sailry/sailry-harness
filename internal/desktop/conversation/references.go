package conversation

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Reference binds a visible mention to its controller-owned target opener.
// Canonical targets remain outside the renderer and its Turn contract.
type Reference struct {
	ID, Label string
	Open      func()
}

type referenceDetail struct {
	session, title, subtitle, body string
	open                           bool
}

// SetReferences replaces only this transcript's view-only mention bindings.
func (s *State) SetReferences(session string, references map[string][]Reference) {
	if s.references == nil {
		s.references = map[string]map[string][]Reference{}
	}
	if len(references) == 0 {
		delete(s.references, session)
	} else {
		s.references[session] = references
	}
}

func (s *State) ShowReference(session, title, subtitle, body string) {
	s.referenceDetail = referenceDetail{session: session, title: title, subtitle: subtitle, body: body, open: true}
}

func (s *State) referenceDialog(c *ui.Context, session string) {
	detail := &s.referenceDetail
	if detail.session == session {
		kit.Details(c.Key("reference-detail"), &detail.open, detail.title, detail.subtitle, detail.body)
	}
}

type referencePiece struct {
	text   string
	start  int
	target *Reference
}

func referencePieces(text string, references []Reference) []referencePiece {
	var pieces []referencePiece
	for offset := 0; offset < len(text); {
		start, size, selected := len(text), 0, -1
		for i, ref := range references {
			if ref.Label == "" {
				continue
			}
			token := "@" + strings.TrimPrefix(ref.Label, "@")
			if index := strings.Index(text[offset:], token); index >= 0 && (offset+index < start || offset+index == start && len(token) > size) {
				start, size, selected = offset+index, len(token), i
			}
		}
		if start > offset {
			pieces = append(pieces, referencePiece{text: text[offset:start], start: offset})
		}
		if selected < 0 {
			break
		}
		pieces = append(pieces, referencePiece{text: text[start : start+size], start: start, target: &references[selected]})
		offset = start + size
	}
	return pieces
}

func sliceSpans(spans []ui.Span, start, end int) []ui.Span {
	var out []ui.Span
	offset := 0
	for _, span := range spans {
		next := offset + len(span.Text)
		from, to := max(start, offset), min(end, next)
		if from < to {
			span.Text = span.Text[from-offset : to-offset]
			out = append(out, span)
		}
		offset = next
	}
	return out
}

func referenceText(c *ui.Context, text string, marks []ui.Span, references []Reference) ui.Element {
	palette := theme.Of(c)
	return ui.RichText(c).Children(func() {
		for i, piece := range referencePieces(text, references) {
			spans := sliceSpans(marks, piece.start, piece.start+len(piece.text))
			if len(spans) == 0 {
				spans = []ui.Span{{Text: piece.text}}
			}
			if piece.target != nil {
				for j := range spans {
					spans[j].Color, spans[j].Weight = palette.Primary, 500
				}
			}
			key := struct {
				Index int
				ID    string
			}{Index: i}
			if piece.target != nil {
				key.ID = piece.target.ID
			}
			inline := ui.RichText(c.Key(key), spans...)
			if piece.target != nil && piece.target.Open != nil {
				inline.Role(ui.RoleLink).Label(piece.target.Label).Focusable().Cursor(ui.CursorPointer).OnClick(piece.target.Open)
			}
		}
	})
}
