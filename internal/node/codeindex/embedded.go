package codeindex

import (
	"context"
	"embed"
	"strings"
	"time"

	"github.com/odvcencio/gotreesitter"
	"github.com/odvcencio/gotreesitter/grammars"
)

const maxRegions = 64

// The injection and tags queries are the ones the upstream grammars publish;
// gotreesitter ships the grammars without them.
//
//go:embed queries
var queries embed.FS

func bundled(kind, language string) (string, bool) {
	data, err := queries.ReadFile("queries/" + kind + "/" + language + ".scm")
	return string(data), err == nil
}

// injections returns the compiled injection query of a language, or nil when
// the language has none or the grammar rejects it.
func (p *Parser) injections(entry *grammars.LangEntry) *gotreesitter.Query {
	p.mutex.Lock()
	defer p.mutex.Unlock()
	key := "injections/" + entry.Name
	if query, ok := p.queries[key]; ok {
		return query
	}
	var query *gotreesitter.Query
	if source, ok := bundled("injections", entry.Name); ok {
		query, _ = gotreesitter.NewQuery(source, entry.Language())
	}
	if p.queries == nil {
		p.queries = map[string]*gotreesitter.Query{}
	}
	p.queries[key] = query
	return query
}

// embedded indexes the regions a host language delegates to other grammars.
func (p *Parser) embedded(ctx context.Context, parsed *Parsed, s *syntax, entry *grammars.LangEntry, tree *gotreesitter.Tree) error {
	if _, ok := bundled("injections", entry.Name); !ok {
		return nil
	}
	query := p.injections(entry)
	if query == nil {
		parsed.Partial = true
		return nil
	}
	deadline := time.Now().Add(parseMicros * time.Microsecond)
	cursor := query.Exec(tree.RootNode(), s.language, []byte(s.source))
	regions := 0
	for {
		matched, ok := cursor.NextMatch()
		if !ok {
			return nil
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if !time.Now().Before(deadline) || regions >= maxRegions {
			parsed.Partial, parsed.Truncated = true, true
			return nil
		}
		language := ""
		for _, capture := range matched.Captures {
			if capture.Name == "injection.language" {
				language = s.text(capture.Node)
				break
			}
		}
		if language == "" {
			if fixed := matched.SetValues(query, "injection.language"); len(fixed) > 0 {
				language = fixed[0]
			}
		}
		for _, capture := range matched.Captures {
			if capture.Name != "injection.content" {
				continue
			}
			if regions >= maxRegions {
				parsed.Partial, parsed.Truncated = true, true
				break
			}
			if language == "" {
				parsed.Partial = true
				continue
			}
			region := capture.Node
			name := language
			if refined := s.refinement(region); refined != "" {
				name = refined
			}
			guest := grammars.DetectLanguageByName(name)
			if guest == nil {
				parsed.Partial = true
				continue
			}
			text := s.text(region)
			guestTree, err := parse(ctx, guest, text)
			if err != nil {
				return err
			}
			if guestTree == nil {
				parsed.Partial = true
			} else {
				parsed.Capabilities.Embedded = true
				inner := &syntax{language: guest.Language(), source: text}
				start := region.StartPoint()
				parsed.collect(extract(inner, guestTree.RootNode(), guest.Name), guest.Name, text,
					origin{byte: int(region.StartByte()), line: int(start.Row), column: int(start.Column)})
				guestTree.Release()
			}
			regions++
		}
	}
}

// refinement reads the grammar a script or style element names.
func (s *syntax) refinement(region node) string {
	host := region.Parent()
	if host == nil {
		return ""
	}
	if kind := s.kind(host); kind != "script_element" && kind != "style_element" {
		return ""
	}
	tag := s.namedOfKind(host, "start_tag")
	if tag == nil {
		return ""
	}
	for _, attribute := range s.named(tag) {
		if s.kind(attribute) != "attribute" {
			continue
		}
		name, value, ok := strings.Cut(s.text(attribute), "=")
		if !ok {
			continue
		}
		if name = strings.ToLower(strings.TrimSpace(name)); name != "lang" && name != "type" {
			continue
		}
		value = strings.Trim(strings.TrimSpace(value), `'"`)
		if suffix, ok := strings.CutPrefix(value, "text/"); ok {
			value = suffix
		} else if suffix, ok := strings.CutPrefix(value, "application/"); ok {
			value = suffix
		}
		switch value {
		case "ts", "typescript":
			return "typescript"
		case "js", "javascript", "module", "ecmascript":
			return "javascript"
		case "ld+json":
			return "json"
		}
		return value
	}
	return ""
}
