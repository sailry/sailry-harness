package codeindex

import (
	"strings"
	"unicode"
)

// item is one declaration with the declarations nested in its body.
type item struct {
	kind      string
	name      string
	named     bool
	span      span
	signature string
	signed    bool
	children  []item
}

// statement is one import statement.
type statement struct {
	source string
	items  []string
	alias  string
	span   span
}

// extraction is everything one parsed source contributes.
type extraction struct {
	structure []item
	symbols   []item
	imports   []statement
	data      *item
	errors    int
}

func isSpace(character rune) bool { return unicode.IsSpace(character) }

// extract reads declarations, imports and data fields from one tree.
func extract(s *syntax, root node, language string) extraction {
	var found extraction
	declarative := language == "sql" || language == "just" || language == "dockerfile" || language == "c"
	quoted := -1
	walk(root, func(n node, depth int) bool {
		if n.IsError() || n.IsMissing() {
			found.errors++
		}
		if !declarative {
			if symbol, ok := s.symbolAt(n, language); ok {
				found.symbols = append(found.symbols, symbol)
			}
		}
		// Elixir quote bodies are generated syntax, not imports.
		if quoted >= 0 && depth <= quoted {
			quoted = -1
		}
		if quoted >= 0 {
			return true
		}
		if language == "elixir" && s.kind(n) == "call" {
			if s.elixirQuote(n) {
				quoted = depth
			} else if s.elixirDirective(n) {
				found.imports = append(found.imports, statement{source: s.text(n), span: s.span(n)})
			}
			return true
		}
		if imported, ok := s.importAt(n, language); ok {
			found.imports = append(found.imports, imported)
		}
		return true
	})
	switch language {
	case "sql":
		found.structure = s.sqlStructure(root)
	case "just":
		found.structure = s.justStructure(root)
	case "c":
		found.structure = s.cStructure(root)
	case "dockerfile":
		found.structure = s.dockerfileStructure(root)
	default:
		found.structure = s.structure(root, language, 0)
	}
	found.data = s.data(root, language)
	return found
}

// structure returns the declarations under n. A declaration nests only what
// its body declares.
func (s *syntax) structure(n node, language string, depth int) []item {
	if language == "elixir" && s.elixirQuote(n) {
		return nil
	}
	kind, name, named, body, matched := "", "", false, node(nil), false
	if language == "elixir" {
		kind, name, named, body, matched = s.elixirDefinition(n)
	}
	if !matched {
		if kind = structureKind(s.kind(n), language, s, n); kind != "" {
			name, named = s.structureName(n)
			body, matched = s.field(n, "body"), true
		}
	}
	if !matched {
		if depth >= maxTreeDepth {
			return nil
		}
		var found []item
		for _, child := range n.Children() {
			found = append(found, s.structure(child, language, depth+1)...)
		}
		return found
	}
	end := int(n.EndByte())
	if body != nil {
		end = int(body.StartByte())
	}
	declared := item{kind: kind, name: name, named: named, span: s.span(n)}
	if start := int(n.StartByte()); start <= end && end <= len(s.source) {
		declared.signature = strings.TrimRightFunc(s.source[start:end], isSpace)
		declared.signed = declared.signature != ""
	}
	if body != nil && depth < maxTreeDepth {
		declared.children = s.structure(body, language, depth+1)
	}
	return []item{declared}
}

func structureKind(kind, language string, s *syntax, n node) string {
	switch kind {
	case "function_definition", "function_declaration", "protocol_function_declaration", "function_item", "arrow_function":
		return "function"
	case "method_definition", "method_declaration":
		return "method"
	case "method", "singleton_method":
		if language == "ruby" {
			return "method"
		}
	case "class_definition", "class_declaration", "class":
		if kind == "class_declaration" && language == "swift" {
			return s.swiftKind(n)
		}
		return "class"
	case "struct_item", "struct_definition", "struct_declaration":
		return "struct"
	case "interface_declaration", "interface_definition", "protocol_declaration":
		return "interface"
	case "enum_item", "enum_definition", "enum_declaration":
		return "enum"
	case "module_definition", "mod_item", "package_header", "package_declaration":
		return "module"
	case "module":
		if language == "ruby" {
			return "module"
		}
	case "internal_module":
		if language == "typescript" || language == "tsx" {
			return "namespace"
		}
	case "trait_item":
		return "trait"
	case "impl_item":
		return "impl"
	}
	return ""
}

// swiftKind separates the declarations Swift folds into one node kind.
func (s *syntax) swiftKind(n node) string {
	declared := s.field(n, "declaration_kind")
	if declared == nil {
		return ""
	}
	switch s.kind(declared) {
	case "struct":
		return "struct"
	case "extension":
		return "impl"
	case "enum":
		return "enum"
	case "class", "actor":
		return "class"
	}
	return ""
}

func same(left, right node) bool {
	return left == right || left != nil && right != nil && left.StartByte() == right.StartByte() &&
		left.EndByte() == right.EndByte() && left.Symbol() == right.Symbol()
}

func (s *syntax) structureName(n node) (string, bool) {
	if s.kind(n) == "arrow_function" {
		value, parent := n, n.Parent()
		for parent != nil && s.kind(parent) == "parenthesized_expression" {
			value, parent = parent, parent.Parent()
		}
		if parent == nil || s.kind(parent) != "variable_declarator" || !same(s.field(parent, "value"), value) {
			return "", false
		}
		name := s.field(parent, "name")
		if name == nil || s.kind(name) != "identifier" {
			return "", false
		}
		return s.text(name), true
	}
	if name := s.field(n, "name"); name != nil {
		if text := s.text(name); text != "" {
			return text, true
		}
	}
	for _, kind := range []string{"type_identifier", "identifier", "scoped_identifier"} {
		for _, child := range s.named(n) {
			if s.kind(child) == kind {
				if text := s.text(child); text != "" {
					return text, true
				}
			}
		}
	}
	return "", false
}

// symbolAt classifies n as a named declaration.
func (s *syntax) symbolAt(n node, language string) (item, bool) {
	declaration, kind := n, ""
	switch s.kind(n) {
	case "function_definition", "function_declaration", "protocol_function_declaration", "function_item":
		kind = "function"
	case "class_definition", "class_declaration":
		kind = "class"
		if s.kind(n) == "class_declaration" && language == "swift" {
			switch s.swiftKind(n) {
			case "struct":
				kind = "type"
			case "enum":
				kind = "enum"
			case "class":
			default:
				return item{}, false
			}
		}
	case "type_alias_declaration", "typealias_declaration", "type_item":
		kind = "type"
	case "type_spec":
		kind = "type"
		if declared := s.field(n, "type"); declared != nil && s.kind(declared) == "interface_type" {
			kind = "interface"
		}
	case "interface_declaration", "protocol_declaration":
		kind = "interface"
	case "enum_item", "enum_declaration":
		kind = "enum"
	case "const_item", "const_declaration":
		kind = "constant"
	case "variable_declarator":
		if language != "javascript" && language != "typescript" && language != "tsx" {
			return item{}, false
		}
		parent := n.Parent()
		if parent == nil || s.kind(parent) != "lexical_declaration" || !s.declarationScope(n) {
			return item{}, false
		}
		if declared := s.field(parent, "kind"); declared == nil || s.kind(declared) != "const" {
			return item{}, false
		}
		value := s.field(n, "value")
		for value != nil && s.kind(value) == "parenthesized_expression" {
			value = value.NamedChild(0)
		}
		if value != nil {
			switch s.kind(value) {
			case "arrow_function", "function_expression", "generator_function":
				return item{}, false
			}
		}
		kind = "constant"
	case "pattern":
		if language != "swift" {
			return item{}, false
		}
		declaration = n.Parent()
		if declaration == nil || s.kind(declaration) != "property_declaration" || !s.declarationScope(n) {
			return item{}, false
		}
		constant := false
		for _, child := range s.named(declaration) {
			constant = constant || s.kind(child) == "value_binding_pattern" && s.text(child) == "let"
		}
		if !constant {
			return item{}, false
		}
		kind = "constant"
	case "let_declaration", "variable_declaration", "lexical_declaration":
		kind = "variable"
	default:
		return item{}, false
	}
	field := "name"
	if s.kind(n) == "pattern" {
		field = "bound_identifier"
	}
	name := s.field(n, field)
	if name == nil || s.kind(n) == "variable_declarator" && s.kind(name) != "identifier" {
		return item{}, false
	}
	return item{kind: kind, name: s.text(name), named: true, span: s.span(declaration)}, true
}

// declarationScope reports whether n sits outside every executable body.
func (s *syntax) declarationScope(n node) bool {
	for parent := n.Parent(); parent != nil; parent = parent.Parent() {
		switch s.kind(parent) {
		case "function_declaration", "function_expression", "generator_function_declaration", "generator_function",
			"arrow_function", "method_definition", "function_body", "lambda_literal", "computed_property":
			return false
		}
	}
	return true
}

func (s *syntax) importAt(n node, language string) (statement, bool) {
	kind := s.kind(n)
	switch language {
	case "python":
		if kind != "import_statement" && kind != "import_from_statement" {
			return statement{}, false
		}
	case "javascript", "typescript", "tsx":
		if kind != "import_statement" {
			return statement{}, false
		}
	case "rust":
		if kind != "use_declaration" {
			return statement{}, false
		}
	case "go":
		if kind != "import_spec" {
			return statement{}, false
		}
	case "java", "kotlin":
		if kind != "import_declaration" {
			return statement{}, false
		}
	default:
		return statement{}, false
	}
	found := statement{source: s.text(n), span: s.span(n)}
	switch language {
	case "python":
		entries := s.fields(n, "name")
		for _, entry := range entries {
			name, alias := s.text(entry), ""
			if s.kind(entry) == "aliased_import" {
				name = ""
				if declared := s.field(entry, "name"); declared != nil {
					name = s.text(declared)
				}
				if declared := s.field(entry, "alias"); declared != nil {
					alias = s.text(declared)
				}
			}
			found.items = append(found.items, name)
			if len(entries) == 1 {
				found.alias = alias
			}
		}
	case "javascript", "typescript", "tsx":
		clause := s.namedOfKind(n, "import_clause")
		if clause == nil {
			break
		}
		for _, child := range s.named(clause) {
			switch s.kind(child) {
			case "namespace_import":
				if name := child.NamedChild(0); name != nil {
					found.alias = s.text(name)
				}
			case "named_imports":
				var specifiers []node
				for _, specifier := range s.named(child) {
					if s.kind(specifier) == "import_specifier" {
						specifiers = append(specifiers, specifier)
					}
				}
				for _, specifier := range specifiers {
					if name := s.field(specifier, "name"); name != nil {
						found.items = append(found.items, s.text(name))
					}
				}
				if len(specifiers) == 1 {
					found.alias = ""
					if alias := s.field(specifiers[0], "alias"); alias != nil {
						found.alias = s.text(alias)
					}
				}
			}
		}
	}
	return found, true
}
