//go:build !sailry_no_code_index

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

// injections distinguishes an absent bundle from a rejected query.
func (p *Parser) injections(name string, language *gotreesitter.Language) compiledQuery {
	p.mutex.Lock()
	defer p.mutex.Unlock()
	key := "injections/" + name
	if compiled, ok := p.queries[key]; ok {
		return compiled
	}
	source, present := bundled("injections", name)
	compiled := compiledQuery{present: present}
	if present {
		compiled.query, _ = gotreesitter.NewQuery(source, language)
	}
	if p.queries == nil {
		p.queries = map[string]compiledQuery{}
	}
	p.queries[key] = compiled
	return compiled
}

// embedded indexes the regions a host language delegates to other grammars.
func (p *Parser) embedded(ctx context.Context, parsed *Parsed, s *syntax, name string, tree *gotreesitter.Tree, sourceOnly bool) error {
	compiled := p.injections(name, s.language)
	if !compiled.present {
		return nil
	}
	query := compiled.query
	if query == nil {
		parsed.Partial = true
		return nil
	}
	deadline := time.Now().Add(parseMicros * time.Microsecond)
	cursor := query.Exec(tree.RootNode(), s.language, tree.Source())
	cursor.SetMatchWorkBudget(matchWorkBudget)
	regions := 0
	for {
		matched, ok := cursor.NextMatch()
		if !ok {
			if cursor.DidExceedMatchLimit() {
				parsed.Partial, parsed.Truncated = true, true
			}
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
			if sourceOnly && !sourceLanguage(guest.Name) {
				continue
			}
			text := s.text(region)
			guestTree, err := parse(ctx, guest, []byte(text))
			if err != nil {
				return err
			}
			if guestTree == nil {
				parsed.Partial = true
			} else {
				parsed.Capabilities.Embedded = true
				parsed.Partial = parsed.Partial || guestTree.ParseStoppedEarly()
				parsed.Truncated = parsed.Truncated || guestTree.ParseStopReason() == gotreesitter.ParseStopMemoryBudget
				inner := &syntax{language: guestTree.Language(), source: text}
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
