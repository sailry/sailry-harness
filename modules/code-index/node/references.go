//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"slices"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/odvcencio/gotreesitter"
)

const (
	maxReferences    = 8192
	maxItems         = 64
	maxReferenceName = 256
)

// additions are calls the published tags queries omit. An addition the
// grammar rejects is skipped.
var additions = map[string][]string{
	"rust": {
		"(call_expression function: (scoped_identifier name: (identifier) @name)) @reference.call",
		"(call_expression function: (generic_function function: (identifier) @name)) @reference.call",
	},
	"c": {
		"(call_expression function: (identifier) @name) @reference.call",
		"(call_expression function: (field_expression field: (field_identifier) @name)) @reference.call",
	},
	"cpp": {
		"(call_expression function: (identifier) @name) @reference.call",
		"(call_expression function: (field_expression field: (field_identifier) @name)) @reference.call",
		"(call_expression function: (qualified_identifier name: (identifier) @name)) @reference.call",
	},
}

// references records the file's imports and the references its tags query finds.
func (p *Parser) references(ctx context.Context, parsed *Parsed, s *syntax, name string, tree *gotreesitter.Tree, imported []statement) error {
	parsed.Imports = modules(name, parsed.Path, imported)
	found := imports(imported)
	if query := p.tags(name, s.language); query != nil {
		var truncated bool
		var err error
		if found, truncated, err = collect(ctx, query, s, tree, aliases(imported), found); err != nil {
			return err
		}
		if truncated {
			parsed.Partial, parsed.Truncated = true, true
		}
	}
	type identity struct {
		start uint32
		name  string
	}
	seen := map[identity]bool{}
	found = slices.DeleteFunc(found, func(usage Usage) bool {
		key := identity{usage.Start, usage.Name}
		duplicate := seen[key]
		seen[key] = true
		return duplicate
	})
	found = found[:min(len(found), maxReferences)]
	enclose(parsed.Symbols, found)
	parsed.References = found
	return nil
}

// tags returns the compiled reference-bearing tags query of a language.
func (p *Parser) tags(name string, language *gotreesitter.Language) *gotreesitter.Query {
	p.mutex.Lock()
	defer p.mutex.Unlock()
	key := "tags/" + name
	if compiled, ok := p.queries[key]; ok {
		return compiled.query
	}
	query := compile(name, language)
	if p.queries == nil {
		p.queries = map[string]compiledQuery{}
	}
	p.queries[key] = compiledQuery{query: query}
	return query
}

func compile(name string, language *gotreesitter.Language) *gotreesitter.Query {
	own, ok := bundled("tags", name)
	if !ok {
		return nil
	}
	var sources []string
	// TypeScript grammars extend JavaScript; their tags omit its calls.
	if name == "typescript" || name == "tsx" {
		if inherited, ok := bundled("tags", "javascript"); ok {
			sources = append(sources, inherited)
		}
	}
	sources = append(append(sources, own), additions[name]...)
	text := ""
	var query *gotreesitter.Query
	for _, source := range sources {
		// Keep whole sources when they compile; otherwise keep each pattern
		// the grammar accepts.
		units := []string{source}
		if compiled, err := gotreesitter.NewQuery(source, language); err != nil {
			units = patterns(source)
		} else if query == nil {
			text, query = source, compiled
			continue
		}
		for _, unit := range units {
			extended := text + "\n" + unit
			if compiled, err := gotreesitter.NewQuery(extended, language); err == nil {
				text, query = extended, compiled
			}
		}
	}
	// Definitions in tags only suppress false references. The outline already
	// extracts declarations even when the tags bundle contains no references.
	for id := range query.CaptureCount() {
		if capture, _ := query.CaptureNameForID(id); strings.HasPrefix(capture, "reference.") {
			return query
		}
	}
	return nil
}

// patterns splits a query source into its top-level patterns, each with its
// captures and predicates.
func patterns(source string) []string {
	var found []string
	depth, start, closed := 0, -1, false
	quoted, escaped, comment := false, false, false
	for index, character := range source {
		if comment {
			comment = character != '\n'
			continue
		}
		if quoted {
			switch {
			case escaped:
				escaped = false
			case character == '\\':
				escaped = true
			case character == '"':
				quoted = false
			}
			continue
		}
		switch character {
		case ';':
			comment = true
		case '"':
			quoted = true
		case '(', '[':
			if depth == 0 && closed {
				if start >= 0 {
					found = append(found, strings.TrimSpace(source[start:index]))
				}
				start, closed = -1, false
			}
			if start < 0 {
				start = index
			}
			depth++
		case ')', ']':
			depth = max(depth-1, 0)
			closed = closed || depth == 0
		}
	}
	if start >= 0 {
		found = append(found, strings.TrimSpace(source[start:]))
	}
	return found
}

// imports lists explicitly imported names as references from the import site.
func imports(imported []statement) []Usage {
	var found []Usage
	for _, statement := range imported {
		for _, entry := range statement.items[:min(len(statement.items), maxItems)] {
			if name := last(entry); identifier(name) {
				found = append(found, Usage{
					Name: name, Kind: "import",
					Line: uint32(statement.span.startLine), Column: uint32(statement.span.startColumn),
					Start: uint32(statement.span.startByte),
				})
			}
		}
	}
	return found
}

func collect(ctx context.Context, query *gotreesitter.Query, s *syntax, tree *gotreesitter.Tree, renamed map[string]string, found []Usage) ([]Usage, bool, error) {
	type reference struct {
		node node
		kind string
	}
	deadline := time.Now().Add(parseMicros * time.Microsecond)
	definitions := map[uint32]bool{}
	var references []reference
	cursor := query.Exec(tree.RootNode(), s.language, tree.Source())
	cursor.SetMatchWorkBudget(matchWorkBudget)
	truncated := false
	for {
		matched, ok := cursor.NextMatch()
		if !ok {
			break
		}
		if err := ctx.Err(); err != nil {
			return nil, false, err
		}
		if !time.Now().Before(deadline) || len(references) >= maxReferences {
			truncated = true
			break
		}
		var name node
		role := ""
		for _, capture := range matched.Captures {
			if name == nil && capture.Name == "name" {
				name = capture.Node
			}
			if role == "" && (strings.HasPrefix(capture.Name, "reference.") || strings.HasPrefix(capture.Name, "definition.")) {
				role = capture.Name
			}
		}
		if name == nil {
			continue
		}
		switch family, kind, _ := strings.Cut(role, "."); family {
		case "definition":
			definitions[name.StartByte()] = true
		case "reference":
			references = append(references, reference{name, kind})
		}
	}
	for _, reference := range references {
		// A definition's own name is not a reference to itself.
		if definitions[reference.node.StartByte()] {
			continue
		}
		text := last(strings.TrimSpace(s.text(reference.node)))
		if !identifier(text) {
			continue
		}
		if original, ok := renamed[text]; ok {
			text = original
		}
		position := reference.node.StartPoint()
		found = append(found, Usage{
			Name: text, Kind: reference.kind,
			Line: position.Row, Column: position.Column, Start: reference.node.StartByte(),
			Receiver: receiver(s.source, int(reference.node.StartByte())),
		})
	}
	return found, truncated || cursor.DidExceedMatchLimit(), nil
}

func identifier(name string) bool {
	if name == "" || len(name) > maxReferenceName {
		return false
	}
	for _, character := range name {
		if !unicode.IsLetter(character) && !unicode.IsNumber(character) && !strings.ContainsRune("_$!?-", character) {
			return false
		}
	}
	return true
}

// receiver reports whether the name follows a self or this member access.
func receiver(source string, start int) bool {
	if start > len(source) {
		return false
	}
	before := strings.TrimRightFunc(source[:start], isSpace)
	for _, prefix := range []string{"self.", "this.", "self::", "Self::", "self->", "this->"} {
		rest, ok := strings.CutSuffix(before, prefix)
		if !ok {
			continue
		}
		previous, _ := utf8.DecodeLastRuneInString(rest)
		if !unicode.IsLetter(previous) && !unicode.IsNumber(previous) && previous != '_' {
			return true
		}
	}
	return false
}

// enclose assigns each reference its innermost containing declaration.
// Declarations are tried before bindings, so a call in `let x = f()` belongs
// to its function rather than to x.
func enclose(symbols []Symbol, found []Usage) {
	slices.SortStableFunc(found, func(left, right Usage) int { return int(left.Start) - int(right.Start) })
	type frame struct {
		symbol *Symbol
		scope  *Scope
	}
	for _, declarations := range []bool{true, false} {
		var open []frame
		next := 0
		for index := range found {
			usage := &found[index]
			for next < len(symbols) && symbols[next].Range.StartByte <= usage.Start {
				symbol := &symbols[next]
				next++
				if kind := symbol.Kind; declarations && !container(kind) || !declarations && kind != "variable" && kind != "constant" {
					continue
				}
				for len(open) > 0 && open[len(open)-1].symbol.Range.EndByte <= symbol.Range.StartByte {
					open = open[:len(open)-1]
				}
				open = append(open, frame{symbol: symbol})
			}
			for len(open) > 0 && open[len(open)-1].symbol.Range.EndByte <= usage.Start {
				open = open[:len(open)-1]
			}
			if usage.Scope == nil && len(open) > 0 {
				top := &open[len(open)-1]
				if top.scope == nil {
					symbol := top.symbol
					top.scope = &Scope{
						Name: symbol.Name, Kind: symbol.Kind,
						StartLine: symbol.Range.StartLine, EndLine: symbol.Range.EndLine,
					}
				}
				usage.Scope = top.scope
			}
		}
	}
}

// container reports whether a declaration kind can hold executable references.
func container(kind string) bool {
	switch kind {
	case "variable", "constant", "data", "field", "property", "parameter", "import", "label":
		return false
	}
	return true
}
