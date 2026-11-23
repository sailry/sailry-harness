//go:build !sailry_no_code_index

package codeindex

import (
	"strings"
	"testing"
)

const (
	denseCalls  = 256
	denseHeader = "package fixture\nfunc run() {\n"
	densePrefix = "\t对象."
	denseLine   = densePrefix + "invoke()\n"
)

func denseGoSource() string {
	return denseHeader + strings.Repeat(denseLine, denseCalls) + "}\n"
}

func parseDenseReferences(t testing.TB, parser *Parser, source string) Parsed {
	t.Helper()
	file, err := parser.Parse(t.Context(), "dense.go", source)
	if err != nil || file.Status != StatusParsed || file.Partial || file.Truncated || len(file.References) != denseCalls {
		t.Fatalf("dense.go: err=%v status=%v partial=%t truncated=%t references=%d", err, file.Status, file.Partial, file.Truncated, len(file.References))
	}
	return file
}

func TestParseDenseReferences(t *testing.T) {
	t.Parallel()
	source := denseGoSource()
	file := parseDenseReferences(t, new(Parser), source)
	scope := Scope{Name: "run", Kind: "function", StartLine: 1, EndLine: denseCalls + 2}
	r := Range{StartByte: uint32(strings.Index(source, "func")), EndByte: uint32(len(source) - 1), StartLine: scope.StartLine, EndLine: scope.EndLine, EndColumn: 1}
	if file.Hash != Hash(source) || len(file.Symbols) != 1 {
		t.Fatalf("dense.go: hash=%q symbols=%d", file.Hash, len(file.Symbols))
	}
	if sym := file.Symbols[0]; sym.FileRevision != file.Hash || sym.Name != scope.Name || sym.Kind != scope.Kind || sym.Range != r {
		t.Fatalf("unexpected declaration: %+v", sym)
	}
	for i, ref := range file.References {
		start := len(denseHeader) + i*len(denseLine) + len(densePrefix)
		if ref.Start != uint32(start) || ref.Line != uint32(i+2) || ref.Column != uint32(len(densePrefix)) || ref.Name != "invoke" || ref.Kind != "call" || ref.Receiver {
			t.Fatalf("reference %d at byte %d: %+v", i, start, ref)
		}
		if ref.Scope == nil || *ref.Scope != scope {
			t.Fatalf("reference %d scope = %+v, want %+v", i, ref.Scope, scope)
		}
	}
}

func TestEncloseRangeBoundaries(t *testing.T) {
	t.Parallel()
	symbol := func(name, kind string, start, end uint32) Symbol {
		return Symbol{Name: name, Kind: kind, Range: Range{StartByte: start, EndByte: end}}
	}
	symbols := []Symbol{
		symbol("outer", "function", 4, 12), symbol("local", "variable", 5, 10),
		symbol("inner", "function", 6, 8), symbol("v", "variable", 12, 14),
		symbol("c", "constant", 14, 16), symbol("field", "field", 16, 20),
	}
	want := []struct {
		start       uint32
		name, scope string
	}{
		{3, "before", ""}, {4, "open", "outer"}, {5, "binding", "outer"},
		{6, "first", "inner"}, {6, "second", "inner"}, {8, "close", "outer"},
		{12, "fallback", "v"}, {14, "adjacent", "c"}, {16, "field", ""},
	}
	var refs []Usage
	for _, i := range []int{7, 3, 0, 6, 8, 5, 2, 1, 4} {
		refs = append(refs, Usage{Start: want[i].start, Name: want[i].name})
	}
	enclose(symbols, refs)
	for i, ref := range refs {
		var scope string
		if ref.Scope != nil {
			scope = ref.Scope.Name
		}
		if ref.Start != want[i].start || ref.Name != want[i].name || scope != want[i].scope {
			t.Fatalf("reference %d: %+v scope=%q, want %+v", i, ref, scope, want[i])
		}
	}
}

func BenchmarkParseDenseReferences(b *testing.B) {
	source := denseGoSource()
	var parser Parser
	parseDenseReferences(b, &parser, source)
	ctx := b.Context()
	b.SetBytes(int64(len(source)))
	b.ReportAllocs()
	for b.Loop() {
		file, err := parser.Parse(ctx, "dense.go", source)
		if err != nil || file.Partial || file.Truncated || len(file.References) != denseCalls {
			b.Fatalf("dense.go: err=%v partial=%t truncated=%t references=%d", err, file.Partial, file.Truncated, len(file.References))
		}
	}
}

func BenchmarkEncloseDenseReferences(b *testing.B) {
	file := parseDenseReferences(b, new(Parser), denseGoSource())
	for i := range file.References {
		file.References[i].Scope = nil
	}
	refs := make([]Usage, len(file.References))
	b.ReportAllocs()
	for b.Loop() {
		copy(refs, file.References)
		enclose(file.Symbols, refs)
	}
}

func BenchmarkOwnLiveParseDenseReferences(b *testing.B) {
	file := parseDenseReferences(b, new(Parser), denseGoSource())
	b.ReportAllocs()
	for b.Loop() {
		owned, _ := ownLiveParse(&file)
		if owned == nil || len(owned.References) != denseCalls {
			b.Fatal("dense parse was not detached")
		}
	}
}
