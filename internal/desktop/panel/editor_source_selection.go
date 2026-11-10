package panel

import (
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown/parser"
	"github.com/egoist/mygo/ui/richtext"
)

type editorSourceSpan struct {
	start, end int
	text       string
}
type editorSourceParagraph struct {
	start, end int
	spans      []editorSourceSpan
}
type editorSourcePoint struct {
	at   int
	span *editorSourceSpan
	rune int
}

// richSourceSelection bridges public parser byte spans and the native document's
// rune selection. Only an entity containing a selection boundary is expanded;
// unrelated Markdown retains its exact source bytes.
func richSourceSelection(source string, document *richtext.Document) (string, editor.Selection, bool) {
	parsed := parser.New(parser.GFM | parser.ExtFootnotes)
	parsed.Set(source)
	paragraphs := []editorSourceParagraph{}
	valid := true
	var blocks func(int)
	blocks = func(index int) {
		block := parsed.Block(index)
		switch block.Kind {
		case parser.Paragraph, parser.Heading:
			paragraph := editorSourceParagraph{start: block.Start, end: block.End}
			for _, inline := range parsed.Inlines(index) {
				switch inline.Kind {
				case parser.Text:
					spans := editorLiteralSpans(source, inline.Start, inline.End)
					var decoded strings.Builder
					for _, span := range spans {
						decoded.WriteString(span.text)
					}
					if decoded.String() != inline.Text {
						valid = false
						return
					}
					paragraph.spans = append(paragraph.spans, spans...)
				case parser.Code:
					raw := source[inline.Start:inline.End]
					fence := len(raw) - len(strings.TrimLeft(raw, "`"))
					body := raw[fence : len(raw)-fence]
					at := strings.Index(body, inline.Text)
					if at < 0 {
						valid = false
						return
					}
					start := inline.Start + fence + at
					paragraph.spans = append(paragraph.spans, editorSourceSpan{start: start, end: start + len(inline.Text), text: inline.Text})
				case parser.SoftBreak:
					paragraph.spans = append(paragraph.spans, editorSourceSpan{start: inline.Start, end: inline.End, text: " "})
				case parser.LineBreak:
					paragraph.spans = append(paragraph.spans, editorSourceSpan{start: inline.Start, end: inline.End, text: "\n"})
				}
			}
			paragraphs = append(paragraphs, paragraph)
		case parser.CodeBlock:
			for line := range parsed.LineCount(index) {
				text, start := parsed.Line(index, line)
				end := start + len(text)
				if end > len(source) || source[start:end] != text {
					valid = false
					return
				}
				paragraphs = append(paragraphs, editorSourceParagraph{start: start, end: end, spans: []editorSourceSpan{{start: start, end: end, text: text}}})
			}
			if parsed.LineCount(index) == 0 {
				start := block.End
				if line := strings.IndexByte(source[block.Start:block.End], '\n'); line >= 0 {
					start = block.Start + line + 1
				}
				paragraphs = append(paragraphs, editorSourceParagraph{start: start, end: start})
			}
		default:
			first := true
			for child := range parsed.Children(index) {
				if first && block.Kind == parser.Item && parsed.Block(child).Kind != parser.Paragraph && parsed.Block(child).Kind != parser.Heading {
					at := block.End
					if line := strings.IndexByte(source[block.Start:block.End], '\n'); line >= 0 {
						at = block.Start + line
					}
					paragraphs = append(paragraphs, editorSourceParagraph{start: at, end: at})
				}
				first = false
				blocks(child)
			}
			if first && block.Kind == parser.Item {
				paragraphs = append(paragraphs, editorSourceParagraph{start: block.End, end: block.End})
			}
		}
	}
	blocks(0)
	var text strings.Builder
	for index, paragraph := range paragraphs {
		if index > 0 {
			text.WriteByte('\n')
		}
		for _, span := range paragraph.spans {
			text.WriteString(span.text)
		}
	}
	if !valid || text.String() != document.Text().String() {
		return source, editor.Selection{}, false
	}
	anchor, caret := document.Selection()
	point := func(offset int, startBoundary bool) editorSourcePoint {
		for _, paragraph := range paragraphs {
			length := 0
			for _, span := range paragraph.spans {
				length += utf8.RuneCountInString(span.text)
			}
			if offset <= length {
				for index := range paragraph.spans {
					span := &paragraph.spans[index]
					count := utf8.RuneCountInString(span.text)
					if offset == count && startBoundary && index+1 < len(paragraph.spans) {
						offset = 0
						continue
					}
					if offset <= count {
						if offset == 0 {
							return editorSourcePoint{at: span.start}
						}
						if offset == count {
							return editorSourcePoint{at: span.end}
						}
						if source[span.start:span.end] == span.text {
							return editorSourcePoint{at: span.start + editorRuneByte(span.text, offset)}
						}
						return editorSourcePoint{at: span.start, span: span, rune: offset}
					}
					offset -= count
				}
				return editorSourcePoint{at: paragraph.end}
			}
			offset -= length + 1
		}
		return editorSourcePoint{at: len(source)}
	}
	from, to := point(anchor, anchor < caret), point(caret, caret < anchor)
	expansions := []editorSourceSpan{}
	for _, point := range []editorSourcePoint{from, to} {
		if point.span != nil && !slices.ContainsFunc(expansions, func(span editorSourceSpan) bool { return span.start == point.span.start }) {
			expansions = append(expansions, *point.span)
		}
	}
	slices.SortFunc(expansions, func(a, b editorSourceSpan) int { return a.start - b.start })
	var expanded strings.Builder
	previous := 0
	for _, span := range expansions {
		expanded.WriteString(source[previous:span.start])
		expanded.WriteString(span.text)
		previous = span.end
	}
	expanded.WriteString(source[previous:])
	resolve := func(point editorSourcePoint) int {
		shift := 0
		for _, span := range expansions {
			if point.span != nil && point.span.start == span.start {
				return span.start + shift + editorRuneByte(span.text, point.rune)
			}
			if span.end <= point.at {
				shift += len(span.text) - (span.end - span.start)
			}
		}
		return point.at + shift
	}
	updated := expanded.String()
	if len(expansions) > 0 && richtext.ParseMarkdown(updated).String() != document.Text().String() {
		return source, editor.Selection{}, false
	}
	return updated, editor.Selection{Anchor: resolve(from), Head: resolve(to)}, true
}

func editorRuneByte(text string, count int) int {
	for at := range text {
		if count == 0 {
			return at
		}
		count--
	}
	return len(text)
}

// Literal decoding stays with MyGo; this adapter only retains each decoded
// token's original byte range for selection boundaries.
func editorLiteralSpans(source string, start, end int) []editorSourceSpan {
	spans := []editorSourceSpan{}
	for at := start; at < end; {
		next := end
		if special := strings.IndexAny(source[at:end], "\\&"); special > 0 {
			next = at + special
		} else if special == 0 {
			next = at + 1
			if source[at] == '\\' && next < end {
				_, width := utf8.DecodeRuneInString(source[next:end])
				next += width
			}
			if source[at] == '&' {
				for next < end && (source[next] >= 'a' && source[next] <= 'z' || source[next] >= 'A' && source[next] <= 'Z' || source[next] >= '0' && source[next] <= '9' || source[next] == '#') {
					next++
				}
				if next < end && source[next] == ';' {
					next++
				} else {
					next = at + 1
				}
			}
		}
		raw := source[at:next]
		decoded := raw
		if raw[0] == '\\' || raw[0] == '&' {
			literal := parser.New(parser.GFM | parser.ExtFootnotes)
			literal.Set("X" + raw)
			for block := range literal.Children(0) {
				decoded = strings.TrimPrefix(parser.PlainText(literal.Inlines(block)), "X")
				break
			}
		}
		spans = append(spans, editorSourceSpan{start: at, end: next, text: decoded})
		at = next
	}
	return spans
}
