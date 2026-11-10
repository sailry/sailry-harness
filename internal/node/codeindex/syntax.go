package codeindex

import (
	"strings"

	"github.com/odvcencio/gotreesitter"
)

// maxTreeDepth bounds every extraction walk.
const maxTreeDepth = 512

type node = *gotreesitter.Node

// span is a source range with UTF-8 byte columns.
type span struct {
	startByte, endByte     int
	startLine, startColumn int
	endLine, endColumn     int
}

// syntax reads one parsed source.
type syntax struct {
	language *gotreesitter.Language
	source   string
}

func (s *syntax) kind(n node) string { return n.Type(s.language) }

func (s *syntax) field(n node, name string) node { return n.ChildByFieldName(name, s.language) }

func (s *syntax) fields(n node, name string) []node {
	var found []node
	for index := range n.ChildCount() {
		if n.FieldNameForChild(index, s.language) == name {
			found = append(found, n.Child(index))
		}
	}
	return found
}

func (s *syntax) named(n node) []node {
	found := make([]node, 0, n.NamedChildCount())
	for _, child := range n.Children() {
		if child.IsNamed() {
			found = append(found, child)
		}
	}
	return found
}

func (s *syntax) namedOfKind(n node, kind string) node {
	for _, child := range n.Children() {
		if child.IsNamed() && s.kind(child) == kind {
			return child
		}
	}
	return nil
}

func (s *syntax) childOfKind(n node, kind string) node {
	for _, child := range n.Children() {
		if s.kind(child) == kind {
			return child
		}
	}
	return nil
}

func (s *syntax) text(n node) string {
	start, end := min(int(n.StartByte()), len(s.source)), min(int(n.EndByte()), len(s.source))
	if start > end {
		return ""
	}
	return s.source[start:end]
}

func (s *syntax) span(n node) span {
	start, end := n.StartPoint(), n.EndPoint()
	return span{
		startByte: int(n.StartByte()), endByte: int(n.EndByte()),
		startLine: int(start.Row), startColumn: int(start.Column),
		endLine: int(end.Row), endColumn: int(end.Column),
	}
}

// spanBetween trims trailing whitespace from start..end and measures the end
// position forward from the node's known start.
func (s *syntax) spanBetween(from node, end int) span {
	start, point := int(from.StartByte()), from.StartPoint()
	result := span{startByte: start, endByte: start, startLine: int(point.Row), startColumn: int(point.Column)}
	result.endLine, result.endColumn = result.startLine, result.startColumn
	if start > end || end > len(s.source) {
		return result
	}
	text := strings.TrimRightFunc(s.source[start:end], isSpace)
	result.endByte = start + len(text)
	if last := strings.LastIndexByte(text, '\n'); last >= 0 {
		result.endLine += strings.Count(text, "\n")
		result.endColumn = len(text) - last - 1
	} else {
		result.endColumn += len(text)
	}
	return result
}

func (s *syntax) spanTrimmed(n node) span { return s.spanBetween(n, int(n.EndByte())) }

func nextNamed(n node) node {
	for sibling := n.NextSibling(); sibling != nil; sibling = sibling.NextSibling() {
		if sibling.IsNamed() {
			return sibling
		}
	}
	return nil
}

// walk visits a subtree in pre-order. visit reports whether to descend.
func walk(root node, visit func(n node, depth int) bool) {
	var descend func(n node, depth int)
	descend = func(n node, depth int) {
		if !visit(n, depth) || depth >= maxTreeDepth {
			return
		}
		for _, child := range n.Children() {
			descend(child, depth+1)
		}
	}
	descend(root, 0)
}
