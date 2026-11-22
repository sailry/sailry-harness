//go:build !sailry_no_code_index

// Package codeindex extracts declarations and references with the pinned
// gotreesitter grammars and serves bounded queries over a rebuildable index.
package codeindex

import (
	"cmp"
	"context"
	"math"
	"path"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"unicode/utf8"

	"github.com/odvcencio/gotreesitter"
	"github.com/odvcencio/gotreesitter/grammars"
)

const (
	maxSymbols        = 4096
	maxNameBytes      = 512
	maxSignatureBytes = 384
	parseMicros       = 1_000_000
	// Bound the parser's scratch and tree memory for an individual file.
	parseMemoryBytes = 64 << 20
	// The cursor can allocate while enumerating a single pattern at
	// one node, before the caller gets a chance to check its deadline.
	matchWorkBudget = 8_192
)

// Parser extracts one file at a time. Compiled queries are shared by every
// parse.
type Parser struct {
	mutex   sync.Mutex
	queries map[string]compiledQuery
}

type compiledQuery struct {
	query   *gotreesitter.Query
	present bool
}

// Omitted describes a file the index holds no declarations for.
func Omitted(file string, status Status) Parsed {
	parsed := omitted(file, status)
	if entry := grammars.DetectLanguage(file); entry != nil {
		parsed.Language = entry.Name
	}
	return parsed
}

// Candidate reports whether a path contributes source or recognized metadata.
func Candidate(file string) bool {
	return grammars.DetectLanguage(file) != nil || path.Ext(file) == "" || excludedCodePath(file)
}

func detect(file, source string) *grammars.LangEntry {
	if entry := grammars.DetectLanguage(file); entry != nil {
		return entry
	}
	line, _, _ := strings.Cut(source, "\n")
	return grammars.DetectLanguageByShebang(line)
}

// Parse extracts the declarations, references and imports of one source. It
// fails only when ctx is cancelled.
func (p *Parser) Parse(ctx context.Context, file, source string) (Parsed, error) {
	return p.parseSource(ctx, file, parseInput{text: source}, allContent)
}

type parseContent uint8

const (
	allContent parseContent = iota
	sourceContent
	outlineContent
)

type parseInput struct {
	text     string
	data     []byte
	revision string
}

func (input *parseInput) hash() string {
	if input.revision == "" {
		input.data = []byte(input.text)
		input.revision = hashBytes(input.data)
	}
	return input.revision
}

func (p *Parser) parseSource(ctx context.Context, file string, input parseInput, content parseContent) (Parsed, error) {
	if err := ctx.Err(); err != nil {
		return Parsed{}, err
	}
	parsed := Parsed{Path: file, Hash: input.hash(), Status: StatusUnsupported}
	source := input.text
	entry := detect(file, source)
	if entry == nil {
		return parsed, nil
	}
	language := entry.Name
	parsed.Language = language
	if content != allContent && (!sourceLanguage(language) || excludedCodePath(file) || minifiedSource(language, source)) {
		return parsed, nil
	}
	tree, err := parse(ctx, entry, input.data)
	if err != nil {
		return Parsed{}, err
	}
	if tree == nil {
		parsed.Status, parsed.Partial = StatusFailed, true
		return parsed, nil
	}
	defer tree.Release()
	s := &syntax{language: tree.Language(), source: source}
	found := extract(s, tree.RootNode(), language)
	parsed.Status = StatusParsed
	parsed.Partial = tree.ParseStoppedEarly()
	parsed.Truncated = tree.ParseStopReason() == gotreesitter.ParseStopMemoryBudget
	parsed.collect(found, language, source, origin{})
	if err := p.embedded(ctx, &parsed, s, language, tree, content != allContent); err != nil {
		return Parsed{}, err
	}
	slices.SortStableFunc(parsed.Symbols, func(left, right Symbol) int {
		return cmp.Or(
			cmp.Compare(left.Range.StartByte, right.Range.StartByte),
			cmp.Compare(right.Range.EndByte, left.Range.EndByte),
			cmp.Compare(left.Name, right.Name),
		)
	})
	type identity struct {
		name       string
		start, end uint32
	}
	seen := map[identity]bool{}
	parsed.Symbols = slices.DeleteFunc(parsed.Symbols, func(symbol Symbol) bool {
		key := identity{symbol.Name, symbol.Range.StartByte, symbol.Range.EndByte}
		duplicate := seen[key]
		seen[key] = true
		return duplicate
	})
	if content != outlineContent {
		if err := p.references(ctx, &parsed, s, language, tree, found.imports); err != nil {
			return Parsed{}, err
		}
	}
	return parsed, ctx.Err()
}

// parse returns no tree when the source exceeds the size or time budget.
func parse(ctx context.Context, entry *grammars.LangEntry, source []byte) (*gotreesitter.Tree, error) {
	if len(source) > MaxFileBytes {
		return nil, nil
	}
	language := entry.Language()
	parser := gotreesitter.NewParser(language)
	parser.SetMemoryBudgetBytes(parseMemoryBytes)
	parser.SetTimeoutMicros(parseMicros)
	var cancelled uint32
	parser.SetCancellationFlag(&cancelled)
	release := context.AfterFunc(ctx, func() { atomic.StoreUint32(&cancelled, 1) })
	defer release()
	var tree *gotreesitter.Tree
	var err error
	if entry.TokenSourceFactory != nil {
		tree, err = parser.ParseWithTokenSource(source, entry.TokenSourceFactory(source, language))
	} else {
		tree, err = parser.Parse(source)
	}
	if cancel := ctx.Err(); cancel != nil {
		tree.Release()
		return nil, cancel
	}
	if err != nil || tree == nil || tree.RootNode() == nil || tree.ParseStopReason() == gotreesitter.ParseStopTimeout {
		tree.Release()
		return nil, nil
	}
	return tree, nil
}

// origin places an embedded region in its host file.
type origin struct {
	byte, line, column int
}

func (o origin) locate(s span) (Range, bool) {
	startColumn, endColumn := s.startColumn, s.endColumn
	if s.startLine == 0 {
		startColumn += o.column
	}
	if s.endLine == 0 {
		endColumn += o.column
	}
	values := [6]int{o.byte + s.startByte, o.byte + s.endByte, o.line + s.startLine, startColumn, o.line + s.endLine, endColumn}
	for _, value := range values {
		if value < 0 || value > math.MaxUint32 {
			return Range{}, false
		}
	}
	return Range{
		StartByte: uint32(values[0]), EndByte: uint32(values[1]),
		StartLine: uint32(values[2]), StartColumn: uint32(values[3]),
		EndLine: uint32(values[4]), EndColumn: uint32(values[5]),
	}, true
}

// collect appends the declarations of one extraction in outline order.
func (p *Parsed) collect(found extraction, language, source string, placed origin) {
	p.Capabilities.Structure = p.Capabilities.Structure || len(found.structure) > 0
	p.Capabilities.Symbols = p.Capabilities.Symbols || len(found.symbols) > 0
	p.Capabilities.Data = p.Capabilities.Data || found.data != nil
	p.Partial = p.Partial || found.errors != 0
	type level struct {
		item  *item
		depth uint16
	}
	var pending []level
	push := func(items []item, depth uint16) {
		for index := len(items) - 1; index >= 0; index-- {
			pending = append(pending, level{&items[index], depth})
		}
	}
	nested := func(depth uint16) uint16 {
		if depth == math.MaxUint16 {
			return depth
		}
		return depth + 1
	}
	if found.data != nil {
		// Data files contribute field declarations, never serialized values.
		pending = append(pending, level{found.data, 0})
		for len(pending) > 0 {
			current := pending[len(pending)-1]
			pending = pending[:len(pending)-1]
			if current.item.named {
				field := *current.item
				field.kind, field.signature, field.signed = "data", field.name+": "+current.item.kind, true
				p.append(field, current.depth, language, source, placed)
			}
			if len(p.Symbols) >= maxSymbols {
				p.Truncated = true
				break
			}
			push(current.item.children, nested(current.depth))
		}
		return
	}
	push(found.structure, 0)
	for len(pending) > 0 {
		current := pending[len(pending)-1]
		pending = pending[:len(pending)-1]
		if current.item.named {
			p.append(*current.item, current.depth, language, source, placed)
		}
		if len(p.Symbols) >= maxSymbols {
			p.Truncated = true
			break
		}
		push(current.item.children, nested(current.depth))
	}
	for _, symbol := range found.symbols {
		p.append(symbol, 0, language, source, placed)
	}
}

func (p *Parsed) append(declared item, depth uint16, language, source string, placed origin) {
	if len(p.Symbols) >= maxSymbols || len(declared.name) > maxNameBytes {
		p.Truncated = true
		return
	}
	located, ok := placed.locate(declared.span)
	start, end := declared.span.startByte, declared.span.endByte
	if !ok || start > end || end > len(source) || !utf8.RuneStart(byteAt(source, start)) ||
		!utf8.RuneStart(byteAt(source, end)) || declared.name == "" {
		p.Partial = true
		return
	}
	signature := declared.signature
	if !declared.signed {
		signature = firstLine(source[start:end])
	}
	signature = short(signature, maxSignatureBytes)
	symbol := Symbol{
		Path: p.Path, Language: language, Name: declared.name, Kind: declared.kind,
		Range: located, FileRevision: p.Hash, Depth: depth,
	}
	if declared.signed || start < end {
		symbol.Signature = &signature
	}
	p.Symbols = append(p.Symbols, symbol)
}

// byteAt returns a rune-start byte at the end of the text.
func byteAt(text string, index int) byte {
	if index >= len(text) {
		return 0
	}
	return text[index]
}

// firstLine returns text up to its first line terminator.
func firstLine(text string) string {
	line, _, _ := strings.Cut(text, "\n")
	return strings.TrimSuffix(line, "\r")
}

// short cuts text to at most limit bytes on a character boundary.
func short(text string, limit int) string {
	end := min(len(text), limit)
	for end > 0 && end < len(text) && !utf8.RuneStart(text[end]) {
		end--
	}
	return text[:end]
}
