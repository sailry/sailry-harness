package codeindex

import (
	"strconv"
	"strings"
	"unicode/utf8"
)

// Data nodes name fields; serialized values never leave the source.
const (
	keyValue = "keyvalue"
	element  = "element"
	sequence = "sequence"
)

// data returns the field tree of a data language, or nil for other languages.
func (s *syntax) data(root node, language string) *item {
	container := item{kind: keyValue, span: s.span(root)}
	switch language {
	case "json", "json5":
		var found *item
		for _, child := range s.named(root) {
			if value, ok := s.jsonValue(child, "", false, 0); ok {
				if found != nil {
					return nil
				}
				found = &value
			}
		}
		if found != nil {
			return found
		}
	case "toml":
		container.children = s.tomlBody(root, 0)
	case "properties":
		for _, child := range s.named(root) {
			if s.kind(child) == "property" {
				container.children = append(container.children, s.firstNamed(child, keyValue, false))
			}
		}
	case "hcl":
		container.children = s.hclBody(root, 0)
	case "kdl":
		container.children = s.kdlNodes(root, 0)
	case "cue":
		container.children = s.cueBody(root, 0)
	case "yaml":
		container.children = s.yamlChildren(root, 0)
	case "ini", "editorconfig":
		container.children = s.iniTop(root, 0)
	case "csv":
		container.kind = sequence
		for row, child := range s.named(root) {
			if s.kind(child) != "row" {
				continue
			}
			cells := keyed(sequence, strconv.Itoa(row), s.span(child))
			for column, cell := range s.named(child) {
				if s.kind(cell) == "field" {
					cells.children = append(cells.children, keyed(sequence, strconv.Itoa(column), s.span(cell)))
				}
			}
			container.children = append(container.children, cells)
		}
	case "nginx":
		container.children = s.nginxBody(root, 0)
	case "caddy":
		container.children = s.caddyBody(root, 0)
	case "xml":
		if top := s.plistTop(root); top != nil {
			container.children = s.plistValue(top, "", false, keyValue, 0).children
			break
		}
		container.kind = element
		container.children = s.xmlChildren(root, 0)
	case "dtd":
		for _, child := range s.named(root) {
			switch s.kind(child) {
			case "ElementDecl", "AttlistDecl", "GEDecl", "PEDecl":
				container.children = append(container.children, s.firstNamed(child, keyValue, false))
			}
		}
	default:
		return nil
	}
	return &container
}

func keyed(kind, key string, range_ span) item {
	return item{kind: kind, name: key, named: true, span: range_}
}

// firstNamed keys n by its first named child.
func (s *syntax) firstNamed(n node, kind string, strip bool) item {
	found := item{kind: kind, span: s.span(n)}
	if first := n.NamedChild(0); first != nil {
		found.name, found.named = s.text(first), true
		if strip {
			found.name = stripQuotes(found.name)
		}
	}
	return found
}

func stripQuotes(text string) string {
	text = strings.TrimSpace(text)
	if len(text) < 2 {
		return text
	}
	if text[0] == '"' && text[len(text)-1] == '"' || text[0] == '\'' && text[len(text)-1] == '\'' {
		return text[1 : len(text)-1]
	}
	return text
}

func (s *syntax) jsonValue(n node, key string, named bool, depth int) (item, bool) {
	if depth >= maxTreeDepth {
		return item{}, false
	}
	found := item{kind: keyValue, name: key, named: named, span: s.span(n)}
	switch s.kind(n) {
	case "object":
		for _, child := range s.named(n) {
			if s.kind(child) == "pair" {
				if value, ok := s.jsonValue(child, "", false, depth+1); ok {
					found.children = append(found.children, value)
				}
			}
		}
	case "array":
		found.kind = sequence
		for index, child := range s.named(n) {
			if value, ok := s.jsonValue(child, strconv.Itoa(index), true, depth+1); ok {
				found.children = append(found.children, value)
			}
		}
	case "pair":
		value := s.field(n, "value")
		if value == nil {
			return item{}, false
		}
		name, named := "", false
		if key := s.field(n, "key"); key != nil {
			name, named = stripQuotes(s.text(key)), true
		}
		pair, ok := s.jsonValue(value, name, named, depth+1)
		pair.span = s.span(n)
		return pair, ok
	case "string", "number", "true", "false", "null":
	default:
		return item{}, false
	}
	return found, true
}

func (s *syntax) tomlBody(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		switch s.kind(child) {
		case "pair":
			if pair, ok := s.tomlPair(child, depth+1); ok {
				found = append(found, pair)
			}
		case "table", "table_array_element":
			named := s.named(child)
			if depth+1 >= maxTreeDepth || len(named) == 0 {
				continue
			}
			table := keyed(keyValue, s.tomlKey(named[0]), s.span(child))
			if s.kind(child) == "table_array_element" {
				table.kind = sequence
			}
			for _, entry := range named[1:] {
				if s.kind(entry) == "pair" {
					if pair, ok := s.tomlPair(entry, depth+2); ok {
						table.children = append(table.children, pair)
					}
				}
			}
			found = append(found, table)
		}
	}
	return found
}

func (s *syntax) tomlPair(n node, depth int) (item, bool) {
	named := s.named(n)
	if depth >= maxTreeDepth || len(named) == 0 {
		return item{}, false
	}
	key := s.tomlKey(named[0])
	if len(named) == 1 {
		return keyed(keyValue, key, s.span(n)), true
	}
	for _, value := range named[1:] {
		if !value.IsExtra() {
			return s.tomlValue(value, key, depth+1)
		}
	}
	return item{}, false
}

func (s *syntax) tomlValue(n node, key string, depth int) (item, bool) {
	if depth >= maxTreeDepth {
		return item{}, false
	}
	found := keyed(keyValue, key, s.span(n))
	switch s.kind(n) {
	case "inline_table":
		found.children = s.tomlBody(n, depth+1)
	case "array":
		found.kind = sequence
		index := 0
		for _, child := range s.named(n) {
			if child.IsExtra() {
				continue
			}
			if value, ok := s.tomlValue(child, strconv.Itoa(index), depth+1); ok {
				found.children = append(found.children, value)
			}
			index++
		}
	}
	return found, true
}

func (s *syntax) tomlKey(n node) string {
	var segments []string
	pending := []node{n}
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if s.kind(current) != "dotted_key" {
			segments = append(segments, stripQuotes(s.text(current)))
			continue
		}
		children := s.named(current)
		for index := len(children) - 1; index >= 0; index-- {
			pending = append(pending, children[index])
		}
	}
	return strings.Join(segments, ".")
}

func (s *syntax) hclBody(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		switch s.kind(child) {
		case "attribute", "pair":
			if child.NamedChildCount() > 0 {
				found = append(found, s.firstNamed(child, keyValue, false))
			}
		case "block":
			named := s.named(child)
			if depth+1 >= maxTreeDepth || len(named) == 0 {
				continue
			}
			var parts []string
			for _, part := range named {
				if kind := s.kind(part); kind == "identifier" || kind == "string_lit" {
					parts = append(parts, stripQuotes(s.text(part)))
				}
			}
			block := keyed(keyValue, strings.Join(parts, "."), s.span(child))
			if len(parts) == 0 {
				block.name = s.text(named[0])
			}
			for _, body := range named {
				if s.kind(body) == "body" {
					block.children = append(block.children, s.hclBody(body, depth+2)...)
				}
			}
			found = append(found, block)
		case "body":
			found = append(found, s.hclBody(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) kdlNodes(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		if s.kind(child) != "node" || depth+1 >= maxTreeDepth {
			continue
		}
		entry := item{kind: keyValue, span: s.span(child)}
		name := s.field(child, "name")
		if name == nil {
			name = s.namedOfKind(child, "identifier")
		}
		if name != nil {
			entry.name, entry.named = s.text(name), true
		}
		if block := s.field(child, "children"); block != nil {
			entry.children = s.kdlNodes(block, depth+2)
		}
		found = append(found, entry)
	}
	return found
}

func (s *syntax) cueBody(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		switch s.kind(child) {
		case "field":
			named := s.named(child)
			if len(named) == 0 {
				continue
			}
			field := item{kind: keyValue, span: s.span(child)}
			for _, part := range named {
				if kind := s.kind(part); kind == "label" || kind == "identifier" || kind == "string" {
					field.name, field.named = stripQuotes(s.text(part)), true
					break
				}
			}
			found = append(found, field)
		case "struct_lit", "source_file":
			found = append(found, s.cueBody(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) yamlChildren(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		switch s.kind(child) {
		case "block_mapping_pair", "flow_pair":
			if pair, ok := s.yamlPair(child, depth+1); ok {
				found = append(found, pair)
			}
		case "block_mapping", "flow_mapping", "document", "block_node", "flow_node", "stream":
			found = append(found, s.yamlChildren(child, depth+1)...)
		case "block_sequence", "flow_sequence":
			found = append(found, s.yamlSequence(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) yamlPair(n node, depth int) (item, bool) {
	if depth >= maxTreeDepth {
		return item{}, false
	}
	key := s.field(n, "key")
	if key == nil {
		return item{}, false
	}
	name, ok := s.yamlKey(key)
	if !ok {
		return item{}, false
	}
	pair := keyed(keyValue, name, s.span(n))
	if value := s.field(n, "value"); value != nil {
		if kind := s.kind(value); kind == "block_node" || kind == "flow_node" {
			pair.children = s.yamlChildren(value, depth+1)
		}
	}
	return pair, true
}

func (s *syntax) yamlKey(n node) (string, bool) {
	switch s.kind(n) {
	case "plain_scalar", "single_quote_scalar", "double_quote_scalar", "block_scalar", "alias":
		return stripQuotes(s.text(n)), true
	case "flow_node", "block_node":
		for _, child := range s.named(n) {
			if key, ok := s.yamlKey(child); ok {
				return key, true
			}
		}
	}
	return "", false
}

func (s *syntax) yamlSequence(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	index := 0
	for _, child := range s.named(n) {
		entry := keyed(sequence, strconv.Itoa(index), s.span(child))
		switch s.kind(child) {
		case "flow_pair":
			if pair, ok := s.yamlPair(child, depth+1); ok {
				entry.children = []item{pair}
			}
		case "block_sequence_item", "flow_node":
			entry.children = s.yamlChildren(child, depth+1)
		default:
			continue
		}
		found = append(found, entry)
		index++
	}
	return found
}

func iniSetting(kind string) bool { return kind == "setting" || kind == "property" || kind == "pair" }

func (s *syntax) iniTop(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		kind := s.kind(child)
		switch {
		case kind == "section":
			section := item{kind: keyValue, span: s.span(child)}
			name := s.namedOfKind(child, "section_name")
			if name == nil {
				name = s.namedOfKind(child, "glob")
			}
			if name != nil {
				section.name, section.named = s.text(name), true
			}
			for _, setting := range s.named(child) {
				if iniSetting(s.kind(setting)) && setting.NamedChildCount() > 0 {
					section.children = append(section.children, s.firstNamed(setting, keyValue, false))
				}
			}
			found = append(found, section)
		case iniSetting(kind):
			if child.NamedChildCount() > 0 {
				found = append(found, s.firstNamed(child, keyValue, false))
			}
		case kind == "preamble":
			found = append(found, s.iniTop(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) nginxBody(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		switch s.kind(child) {
		case "directive":
			if child.NamedChildCount() > 0 {
				found = append(found, s.firstNamed(child, keyValue, false))
			}
		case "block", "http", "events", "server", "location", "map", "if":
			found = append(found, s.nginxBody(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) caddyBody(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		kind := s.kind(child)
		switch {
		case strings.HasPrefix(kind, "directive_") || kind == "directive":
			directive := keyed(keyValue, "", s.span(child))
			if name := s.field(child, "directive_type"); name != nil {
				directive.name = s.text(name)
			} else if first := child.NamedChild(0); first != nil {
				directive.name = s.text(first)
			}
			found = append(found, directive)
		case kind == "server" || kind == "route" || kind == "block":
			found = append(found, s.caddyBody(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) xmlChildren(n node, depth int) []item {
	var found []item
	if depth >= maxTreeDepth {
		return found
	}
	for _, child := range s.named(n) {
		switch s.kind(child) {
		case "element":
			if depth+1 >= maxTreeDepth {
				continue
			}
			entry := item{kind: element, span: s.span(child)}
			if name := s.xmlTag(child); name != "" {
				entry.name, entry.named = name, true
			}
			for _, content := range s.named(child) {
				if s.kind(content) == "content" {
					entry.children = append(entry.children, s.xmlChildren(content, depth+2)...)
				}
			}
			found = append(found, entry)
		case "document", "content":
			found = append(found, s.xmlChildren(child, depth+1)...)
		}
	}
	return found
}

func (s *syntax) xmlTag(n node) string {
	for _, child := range s.named(n) {
		if kind := s.kind(child); kind == "STag" || kind == "EmptyElemTag" {
			if name := s.namedOfKind(child, "Name"); name != nil {
				return s.text(name)
			}
			return ""
		}
	}
	return ""
}

func (s *syntax) xmlElements(n node) []node {
	var found []node
	if content := s.namedOfKind(n, "content"); content != nil {
		for _, child := range s.named(content) {
			if s.kind(child) == "element" {
				found = append(found, child)
			}
		}
	}
	return found
}

// xmlText joins an element's character data, as a property list key needs.
func (s *syntax) xmlText(n node) string {
	content := s.namedOfKind(n, "content")
	if content == nil {
		return ""
	}
	var text strings.Builder
	for _, part := range s.named(content) {
		switch s.kind(part) {
		case "CharData", "CData":
			text.WriteString(s.text(part))
		case "EntityRef", "CharRef":
			text.WriteString(xmlReference(s.text(part)))
		case "CDSect":
			for _, inner := range s.named(part) {
				if s.kind(inner) == "CData" {
					text.WriteString(s.text(inner))
				}
			}
		}
	}
	return text.String()
}

// plistTop returns the top value of an Apple property list.
func (s *syntax) plistTop(root node) node {
	top := s.field(root, "root")
	if top == nil || s.xmlTag(top) != "plist" {
		return nil
	}
	if elements := s.xmlElements(top); len(elements) > 0 {
		return elements[0]
	}
	return nil
}

func (s *syntax) plistValue(n node, key string, named bool, kind string, depth int) item {
	value := item{kind: kind, name: key, named: named, span: s.span(n)}
	if depth+1 >= maxTreeDepth {
		return value
	}
	elements := s.xmlElements(n)
	switch s.xmlTag(n) {
	case "dict":
		for index := 0; index+1 < len(elements); {
			entry := elements[index]
			if s.xmlTag(entry) != "key" {
				index++
				continue
			}
			child := s.plistValue(elements[index+1], strings.TrimSpace(s.xmlText(entry)), true, keyValue, depth+1)
			start := s.span(entry)
			child.span.startByte, child.span.startLine, child.span.startColumn = start.startByte, start.startLine, start.startColumn
			value.children = append(value.children, child)
			index += 2
		}
	case "array":
		for index, entry := range elements {
			value.children = append(value.children, s.plistValue(entry, strconv.Itoa(index), true, sequence, depth+1))
		}
	}
	return value
}

func xmlReference(reference string) string {
	switch reference {
	case "&amp;":
		return "&"
	case "&lt;":
		return "<"
	case "&gt;":
		return ">"
	case "&quot;":
		return `"`
	case "&apos;":
		return "'"
	}
	digits, prefixed := strings.CutPrefix(reference, "&#")
	digits, suffixed := strings.CutSuffix(digits, ";")
	if !prefixed || !suffixed {
		return reference
	}
	base := 10
	if len(digits) > 0 && (digits[0] == 'x' || digits[0] == 'X') {
		digits, base = digits[1:], 16
	}
	value, err := strconv.ParseUint(digits, base, 32)
	if err != nil || !utf8.ValidRune(rune(value)) {
		return reference
	}
	return string(rune(value))
}
