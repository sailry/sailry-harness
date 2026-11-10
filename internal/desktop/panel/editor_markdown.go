package panel

import (
	"regexp"
	"strings"

	"github.com/egoist/mygo/ui/editor"
	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/extension"
	"github.com/yuin/goldmark/text"
)

var editorBlockPrefix = regexp.MustCompile(`^(#{1,6} |[-+*] (?:\[[ xX]\] )?|[0-9]+[.)] |> )`)

// richMarkdownSafe excludes structures the pinned rich-text codec cannot retain.
func richMarkdownSafe(source string) bool {
	if strings.Contains(source, "$$") || strings.Contains(source, `\[`) {
		return false
	}
	data := []byte(source)
	document := goldmark.New(goldmark.WithExtensions(extension.GFM, extension.Footnote)).Parser().Parse(text.NewReader(data))
	safe := true
	_ = ast.Walk(document, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		if !entering {
			return ast.WalkContinue, nil
		}
		switch value := node.(type) {
		case *ast.Document, *ast.Text, *ast.String, *ast.Emphasis, *ast.ListItem, *ast.Blockquote, *ast.Heading, *ast.CodeSpan, *ast.CodeBlock, *ast.AutoLink:
		case *ast.Paragraph, *ast.TextBlock:
			safe = node.Lines().Len() <= 1
		case *ast.List:
			safe = !value.IsOrdered() || value.Start <= 1
		case *ast.Link:
			safe = len(value.Title) == 0
		case *ast.Image:
			safe = false
		case *ast.FencedCodeBlock:
			safe = len(value.Language(data)) == 0
		default:
			safe = node.Kind().String() == "Strikethrough" || node.Kind().String() == "TaskCheckBox"
		}
		if !safe {
			return ast.WalkStop, nil
		}
		return ast.WalkContinue, nil
	})
	return safe
}

func editMarkdownSource(source string, selection editor.Selection, action, value string) (string, editor.Selection) {
	start, end := selection.Start(), selection.End()
	selected := source[start:end]
	if strings.HasPrefix(action, "insert:") {
		snippet := markdownInsertion(strings.TrimPrefix(action, "insert:"), value)
		if start > 0 && source[start-1] != '\n' {
			snippet = "\n\n" + snippet
		}
		if end < len(source) && source[end] != '\n' {
			snippet += "\n\n"
		}
		at := start + len(snippet)
		return source[:start] + snippet + source[end:], editor.Selection{Anchor: at, Head: at}
	}
	marks := map[string]string{"bold": "**", "italic": "*", "strike": "~~", "inline_code": "`"}
	if mark, ok := marks[action]; ok {
		if strings.HasPrefix(selected, mark) && strings.HasSuffix(selected, mark) && len(selected) >= 2*len(mark) {
			text := selected[len(mark) : len(selected)-len(mark)]
			return source[:start] + text + source[end:], editor.Selection{Anchor: start, Head: start + len(text)}
		}
		if start >= len(mark) && end+len(mark) <= len(source) && source[start-len(mark):start] == mark && source[end:end+len(mark)] == mark {
			return source[:start-len(mark)] + selected + source[end+len(mark):], editor.Selection{Anchor: start - len(mark), Head: end - len(mark)}
		}
		return source[:start] + mark + selected + mark + source[end:], editor.Selection{Anchor: start + len(mark), Head: end + len(mark)}
	}
	if action == "link" {
		text := "[" + selected + "](" + value + ")"
		return source[:start] + text + source[end:], editor.Selection{Anchor: start + 1, Head: start + 1 + len(selected)}
	}
	lineStart := strings.LastIndexByte(source[:start], '\n') + 1
	lineEnd := end
	if end > start && source[end-1] == '\n' {
		lineEnd--
	} else if next := strings.IndexByte(source[end:], '\n'); next >= 0 {
		lineEnd = end + next
	} else {
		lineEnd = len(source)
	}
	body := source[lineStart:lineEnd]
	if action == "block:code" {
		text := "```\n" + body + "\n```"
		return source[:lineStart] + text + source[lineEnd:], editor.Selection{Anchor: lineStart + 4, Head: lineStart + 4 + len(body)}
	}
	if action == "block:callout" {
		lines := strings.Split(body, "\n")
		for index := range lines {
			lines[index] = "> " + editorBlockPrefix.ReplaceAllString(lines[index], "")
		}
		text := "> [!NOTE]\n" + strings.Join(lines, "\n")
		return source[:lineStart] + text + source[lineEnd:], editor.Selection{Anchor: lineStart, Head: lineStart + len(text)}
	}
	block := strings.TrimPrefix(action, "block:")
	prefix := map[string]string{"paragraph": "", "bullet": "- ", "ordered": "1. ", "task": "- [ ] ", "quote": "> "}[block]
	if strings.HasPrefix(block, "heading") {
		level := strings.TrimPrefix(block, "heading")
		if len(level) == 1 && level[0] >= '1' && level[0] <= '6' {
			prefix = strings.Repeat("#", int(level[0]-'0')) + " "
		}
	}
	lines := strings.Split(body, "\n")
	for index, line := range lines {
		lines[index] = prefix + editorBlockPrefix.ReplaceAllString(line, "")
	}
	text := strings.Join(lines, "\n")
	return source[:lineStart] + text + source[lineEnd:], editor.Selection{Anchor: lineStart, Head: lineStart + len(text)}
}

func markdownInsertion(kind, value string) string {
	switch kind {
	case "table":
		return "|  |  |\n| --- | --- |\n|  |  |"
	case "image":
		return "![](" + value + ")"
	case "rule":
		return "---"
	case "code":
		return "```\n\n```"
	case "mermaid":
		return "```mermaid\nflowchart LR\n  A --> B\n```"
	case "math":
		return "$$\n\n$$"
	case "chart":
		return "```chart\n{\"type\":\"bar\",\"data\":{\"labels\":[],\"datasets\":[]}}\n```"
	case "html":
		return "```html\n<div>\n\n</div>\n```"
	}
	return value
}
