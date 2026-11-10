package codeindex

import (
	"sort"
	"strings"
)

// sqlStructure reads the CREATE statements of the registry's SQL grammar,
// which names each statement node after its object.
func (s *syntax) sqlStructure(root node) []item {
	var found []item
	walk(root, func(n node, _ int) bool {
		kind, ok := sqlKinds[s.kind(n)]
		if !ok {
			return true
		}
		if name, ok := s.sqlName(n); ok {
			end := int(n.EndByte())
			if next := n.NextSibling(); next != nil && s.kind(next) == ";" {
				end = int(next.EndByte())
			}
			found = append(found, item{kind: kind, name: name, named: true, span: s.spanBetween(n, end)})
		}
		return false
	})
	sort.SliceStable(found, func(left, right int) bool { return found[left].span.startByte < found[right].span.startByte })
	return found
}

var sqlKinds = map[string]string{
	"create_table_statement":             "struct",
	"create_view_statement":              "struct",
	"create_materialized_view_statement": "struct",
	"create_function_statement":          "function",
	"create_procedure_statement":         "function",
	"create_type_statement":              "type",
	"create_domain_statement":            "type",
	"create_index_statement":             "index",
	"create_trigger_statement":           "trigger",
	"create_policy_statement":            "policy",
	"create_schema_statement":            "module",
	"create_database_statement":          "module",
	"create_role_statement":              "role",
}

// sqlName returns the statement's first object name. Triggers and policies
// are qualified by their table.
func (s *syntax) sqlName(statement node) (string, bool) {
	var names []string
	for _, child := range s.named(statement) {
		switch s.kind(child) {
		case "identifier", "dotted_name":
			names = append(names, sqlPlain(s.text(child)))
		}
	}
	if len(names) == 0 {
		return "", false
	}
	switch s.kind(statement) {
	case "create_trigger_statement", "create_policy_statement":
		if len(names) > 1 {
			return names[1] + "." + names[0], true
		}
	}
	return names[0], true
}

func sqlPlain(name string) string {
	parts := strings.Split(name, ".")
	for index, part := range parts {
		if len(part) >= 2 && strings.HasPrefix(part, `"`) && strings.HasSuffix(part, `"`) {
			parts[index] = part[1 : len(part)-1]
		}
	}
	return strings.Join(parts, ".")
}

// cStructure reads C declarations, including prototypes and macros.
func (s *syntax) cStructure(root node) []item {
	var found []item
	add := func(kind string, name, n node, end int) {
		found = append(found, item{kind: kind, name: s.text(name), named: true, span: s.spanBetween(n, end)})
	}
	aggregate := func(n node) {
		kind := ""
		switch s.kind(n) {
		case "struct_specifier":
			kind = "struct"
		case "union_specifier":
			kind = "union"
		case "enum_specifier":
			kind = "enum"
		default:
			return
		}
		name := s.field(n, "name")
		if name == nil || s.field(n, "body") == nil {
			return
		}
		end := int(n.EndByte())
		if next := n.NextSibling(); next != nil && s.kind(next) == ";" {
			end = int(next.EndByte())
		}
		add(kind, name, n, end)
	}
	defined := func(n node) {
		if specifier := s.field(n, "type"); specifier != nil {
			aggregate(specifier)
		}
	}
	walk(root, func(n node, _ int) bool {
		switch s.kind(n) {
		case "translation_unit", "preproc_ifdef", "preproc_if", "preproc_elif", "preproc_elifdef", "preproc_else",
			"linkage_specification", "declaration_list", "ERROR":
			return true
		case "function_definition":
			if declarator := s.field(n, "declarator"); declarator != nil {
				if name := s.cIdentifier(declarator); name != nil {
					add("function", name, n, int(n.EndByte()))
				}
			}
		case "declaration":
			for _, declarator := range s.fields(n, "declarator") {
				if name := s.cPrototype(declarator); name != nil {
					add("function", name, n, int(n.EndByte()))
				}
			}
			defined(n)
		case "type_definition":
			for _, declarator := range s.fields(n, "declarator") {
				if name := s.cIdentifier(declarator); name != nil {
					add("type", name, n, int(n.EndByte()))
				}
			}
			defined(n)
		case "struct_specifier", "union_specifier", "enum_specifier":
			aggregate(n)
		case "preproc_def", "preproc_function_def":
			if name := s.field(n, "name"); name != nil {
				kind := "macro"
				if s.kind(n) == "preproc_def" {
					kind = "constant"
				}
				add(kind, name, n, int(n.EndByte()))
			}
		}
		return false
	})
	sort.SliceStable(found, func(left, right int) bool { return found[left].span.startByte < found[right].span.startByte })
	return found
}

func declaratorLike(kind string) bool {
	return strings.HasSuffix(kind, "declarator") || strings.HasSuffix(kind, "identifier")
}

func (s *syntax) cIdentifier(n node) node {
	pending := []node{n}
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		switch s.kind(current) {
		case "identifier", "type_identifier", "field_identifier":
			return current
		}
		if inner := s.field(current, "declarator"); inner != nil {
			pending = append(pending, inner)
			continue
		}
		children := s.named(current)
		for index := len(children) - 1; index >= 0; index-- {
			if declaratorLike(s.kind(children[index])) {
				pending = append(pending, children[index])
			}
		}
	}
	return nil
}

func (s *syntax) cSole(n node) node {
	for _, child := range s.named(n) {
		if kind := s.kind(child); strings.HasSuffix(kind, "declarator") || kind == "identifier" {
			return child
		}
	}
	return nil
}

func (s *syntax) cPrototype(declarator node) node {
	current := declarator
	for current != nil {
		switch s.kind(current) {
		case "pointer_declarator", "parenthesized_declarator":
			inner := s.field(current, "declarator")
			if inner == nil {
				inner = s.cSole(current)
			}
			current = inner
		case "function_declarator":
			inner := s.field(current, "declarator")
			if inner == nil {
				return nil
			}
			switch s.kind(inner) {
			case "identifier":
				return inner
			case "parenthesized_declarator":
				sole := s.cSole(inner)
				if sole == nil {
					return nil
				}
				if s.kind(sole) == "identifier" {
					return sole
				}
				current = sole
			default:
				current = inner
			}
		default:
			return nil
		}
	}
	return nil
}

// dockerfileStructure groups build arguments and variables by named stage.
func (s *syntax) dockerfileStructure(root node) []item {
	var found []item
	var stage *item
	var first node
	end := 0
	closeStage := func() {
		if stage != nil {
			stage.span = s.spanBetween(first, end)
			found = append(found, *stage)
			stage = nil
		}
	}
	push := func(name, instruction node) {
		constant := item{kind: "constant", name: s.text(name), named: true, span: s.spanTrimmed(instruction)}
		if stage != nil {
			stage.children = append(stage.children, constant)
		} else {
			found = append(found, constant)
		}
	}
	extend := func(instruction node) {
		if stage != nil {
			end = max(end, int(instruction.EndByte()))
		}
	}
	walk(root, func(n node, _ int) bool {
		kind := s.kind(n)
		switch {
		case kind == "from_instruction":
			closeStage()
			if alias := s.field(n, "as"); alias != nil {
				stage, first, end = &item{kind: "module", name: s.text(alias), named: true}, n, int(n.EndByte())
			}
		case kind == "arg_instruction":
			if name := s.field(n, "name"); name != nil {
				push(name, n)
			}
			extend(n)
		case kind == "env_instruction":
			for _, pair := range s.named(n) {
				if s.kind(pair) == "env_pair" {
					if name := s.field(pair, "name"); name != nil {
						push(name, n)
					}
				}
			}
			extend(n)
		case kind == "source_file" || kind == "ERROR":
			return true
		case kind == "comment":
		case strings.HasSuffix(kind, "_instruction"):
			extend(n)
		}
		return false
	})
	closeStage()
	return found
}

// justStructure reads recipes, aliases and modules.
func (s *syntax) justStructure(root node) []item {
	var found []item
	walk(root, func(n node, _ int) bool {
		kind, name := "", node(nil)
		switch s.kind(n) {
		case "recipe":
			kind = "function"
			if header := s.namedOfKind(n, "recipe_header"); header != nil {
				name = s.field(header, "name")
			}
		case "alias":
			kind, name = "alias", s.field(n, "left")
		case "module":
			kind, name = "module", s.field(n, "name")
		default:
			return true
		}
		if name != nil {
			found = append(found, item{kind: kind, name: s.text(name), named: true, span: s.spanTrimmed(n)})
		}
		return false
	})
	return found
}

func (s *syntax) elixirTarget(n node) string {
	if s.kind(n) != "call" {
		return ""
	}
	target := s.field(n, "target")
	if target == nil || s.kind(target) != "identifier" {
		return ""
	}
	return s.text(target)
}

func (s *syntax) elixirQuote(n node) bool { return s.elixirTarget(n) == "quote" }

func (s *syntax) elixirDirective(n node) bool {
	switch s.elixirTarget(n) {
	case "import", "alias", "require", "use":
		return true
	}
	return false
}

// elixirDefinition classifies a definition macro call.
func (s *syntax) elixirDefinition(n node) (kind, name string, named bool, body node, matched bool) {
	bodied := true
	switch s.elixirTarget(n) {
	case "defmodule":
		kind = "module"
	case "defprotocol":
		kind = "interface"
	case "defimpl":
		kind = "impl"
	case "defstruct":
		kind, bodied = "struct", false
	case "def", "defdelegate", "defguard", "defp", "defguardp":
		kind = "function"
	case "defmacro", "defmacrop":
		kind = "macro"
	default:
		return "", "", false, nil, false
	}
	arguments := s.childOfKind(n, "arguments")
	switch kind {
	case "module", "interface", "impl":
		if arguments != nil {
			for _, child := range s.named(arguments) {
				switch s.kind(child) {
				case "alias", "identifier", "dot":
					if text := s.text(child); text != "" && !named {
						name, named = text, true
					}
				}
			}
		}
	case "struct":
	default:
		if arguments != nil {
			name, named = s.elixirCallable(arguments)
		}
	}
	if bodied {
		body = s.elixirBody(n, arguments)
	}
	return kind, name, named, body, true
}

func (s *syntax) elixirCallable(arguments node) (string, bool) {
	for _, child := range s.named(arguments) {
		head := child
		if s.kind(child) == "binary_operator" {
			operator := s.field(child, "operator")
			if operator == nil || s.text(operator) != "when" {
				continue
			}
			if head = s.field(child, "left"); head == nil {
				continue
			}
		}
		if s.kind(head) == "call" {
			if target := s.field(head, "target"); target != nil && s.kind(target) == "identifier" {
				if text := s.text(target); text != "" {
					return text, true
				}
			}
		}
		if s.kind(head) == "identifier" {
			if text := s.text(head); text != "" {
				return text, true
			}
		}
	}
	return "", false
}

func (s *syntax) elixirBody(call, arguments node) node {
	if block := s.childOfKind(call, "do_block"); block != nil {
		return block
	}
	if arguments == nil {
		return nil
	}
	keywords := s.childOfKind(arguments, "keywords")
	if keywords == nil {
		if list := s.childOfKind(arguments, "list"); list != nil {
			keywords = s.childOfKind(list, "keywords")
		}
	}
	if keywords == nil {
		return nil
	}
	for _, pair := range s.named(keywords) {
		if s.kind(pair) != "pair" {
			continue
		}
		if key := s.field(pair, "key"); key != nil && strings.TrimRight(strings.TrimSpace(s.text(key)), ":") == "do" {
			return s.field(pair, "value")
		}
	}
	return nil
}
