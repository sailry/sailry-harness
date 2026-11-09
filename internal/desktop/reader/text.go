package reader

import (
	"bytes"
	"strings"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/text"
	"github.com/yuin/goldmark/util"
)

// Text preserves spoken words and code, without Markdown syntax or link targets.
func Text(markdown string) string {
	source := []byte(markdown)
	document := goldmark.DefaultParser().Parse(text.NewReader(source))
	var spoken strings.Builder
	ast.Walk(document, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			switch node := node.(type) {
			case *ast.Text:
				value := node.Segment.Value(source)
				if !node.IsRaw() {
					value = util.ResolveEntityNames(util.ResolveNumericReferences(util.UnescapePunctuations(value)))
				}
				spoken.Write(value)
				if node.SoftLineBreak() || node.HardLineBreak() {
					spoken.WriteByte('\n')
				}
			case *ast.String:
				spoken.Write(node.Value)
			case *ast.CodeSpan:
				for child := node.FirstChild(); child != nil; child = child.NextSibling() {
					segment := child.(*ast.Text).Segment
					value := segment.Value(source)
					if bytes.HasSuffix(value, []byte("\n")) {
						spoken.Write(value[:len(value)-1])
						spoken.WriteByte(' ')
					} else {
						spoken.Write(value)
					}
				}
				return ast.WalkSkipChildren, nil
			case *ast.CodeBlock, *ast.FencedCodeBlock:
				lines := node.Lines()
				for i := 0; i < lines.Len(); i++ {
					segment := lines.At(i)
					spoken.Write(segment.Value(source))
				}
				return ast.WalkSkipChildren, nil
			case *ast.AutoLink:
				spoken.Write(node.Label(source))
				return ast.WalkSkipChildren, nil
			case *ast.HTMLBlock, *ast.RawHTML:
				return ast.WalkSkipChildren, nil
			}
		} else {
			switch node.(type) {
			case *ast.Paragraph, *ast.Heading, *ast.ListItem, *ast.CodeBlock, *ast.FencedCodeBlock:
				spoken.WriteByte('\n')
			}
		}
		return ast.WalkContinue, nil
	})
	return spoken.String()
}
