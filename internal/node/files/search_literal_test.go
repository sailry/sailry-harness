package files

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLiteralSearchPreservesResults(t *testing.T) {
	t.Parallel()
	root, files := filesFixture(t)
	for name, content := range map[string]string{
		".gitignore":      "ignored.txt\n",
		"a-lines.txt":     "\xef\xbb\xbfzero\r\none\r\ntwo\nNeed.le\r\nbetween\nneed.le and need.le\nend\r",
		"b-binary.txt":    "need.le\n" + strings.Repeat("text\n", boundedReadChunkSize/4) + "\x01\n",
		"c-gb.txt":        "need.le\n" + strings.Repeat("text\n", boundedReadChunkSize/4) + "\xd6\xd0\xce\xc4\n",
		"d-boundary.txt":  "before\n" + strings.Repeat("x", boundedReadChunkSize-3) + "NEED.LE" + strings.Repeat("y", 2*boundedReadChunkSize) + "\r\nafter",
		"e-utf8.txt":      strings.Repeat("x", searchLineByteLimit-1) + "\u754c" + strings.Repeat("x", boundedReadChunkSize-searchLineByteLimit-4) + "\u5339\u914d\n",
		"f-fold.txt":      "before\n\u212a and \u017f\nend\n",
		"g-control.txt":   "need.le\n\u0085\n",
		"h-cr.txt":        strings.Repeat("x", boundedReadChunkSize-1) + "\r\nlast\rcarriage\r",
		"i-utf8-tail.txt": "need.le\n" + strings.Repeat("text\n", boundedReadChunkSize/4) + "NEED.LE \u754c \u212a\n",
		"ignored.txt":     "need.le\n",
		"excluded.md":     "need.le\n",
	} {
		if err := os.WriteFile(filepath.Join(root, name), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	for _, tc := range []struct {
		name      string
		query     string
		sensitive bool
		mode      protocol.SearchMode
		context   uint64
		offset    uint64
		limit     uint64
	}{
		{"context", "need.le", false, protocol.SearchModeContent, 2, 0, 100},
		{"page", "need.le", false, protocol.SearchModeContent, 2, 1, 3},
		{"count", "need.le", false, protocol.SearchModeCount, 0, 0, 100},
		{"files", "need.le", false, protocol.SearchModeFiles, 0, 1, 2},
		{"sensitive", "need.le", true, protocol.SearchModeContent, 0, 0, 100},
		{"utf8 boundary", "\u5339\u914d", true, protocol.SearchModeContent, 1, 0, 100},
		{"unicode fold", "k", false, protocol.SearchModeContent, 1, 0, 100},
		{"unicode fold paged", "k", false, protocol.SearchModeContent, 1, 2, 2},
		{"trailing CR", "\r", true, protocol.SearchModeContent, 1, 0, 100},
		{"wide context", "need.le", false, protocol.SearchModeContent, literalContextLimit + 1, 0, 100},
	} {
		t.Run(tc.name, func(t *testing.T) {
			input := protocol.FileSearch{Query: tc.query, CaseSensitive: tc.sensitive, Mode: tc.mode, Context: tc.context, Offset: tc.offset, Limit: tc.limit, Globs: []string{"*.txt"}}
			got, err := Search(t.Context(), files, input)
			if err != nil {
				t.Fatal(err)
			}
			input.Regex, input.Query = true, regexp.QuoteMeta(input.Query)
			want, err := Search(t.Context(), files, input)
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("literal result differs from decoded result: scanned/skipped=%d/%d, rows=%d, paths=%v, counts=%v, offset=%v, truncated=%t; want %d/%d, rows=%d, paths=%v, counts=%v, offset=%v, truncated=%t", got.ScannedFiles, got.Skipped, len(got.Matches), got.Paths, got.Counts, got.NextOffset, got.Truncated, want.ScannedFiles, want.Skipped, len(want.Matches), want.Paths, want.Counts, want.NextOffset, want.Truncated)
			}
		})
	}
}

func TestLiteralScanUTF8FoldAcrossBuffer(t *testing.T) {
	t.Parallel()
	const word = "\u212a\u017f\u212a"
	scanner := newLiteralSearch(protocol.FileSearch{Query: "kSk", Mode: protocol.SearchModeContent, Context: 1})
	want := []protocol.SearchMatch{
		{LineNumber: 1, Line: "before"},
		{LineNumber: 2, Line: strings.Repeat("x", searchLineByteLimit), Matched: true, Truncated: true},
		{LineNumber: 3, Line: "after \u754c"},
	}
	for split := 1; split < len(word); split++ {
		// Put the buffer boundary at each byte of the folded match, not
		// merely between runes. Its raw width is larger than the needle.
		input := "before\n" + strings.Repeat("x", boundedReadChunkSize-split) + word + "\r\nafter \u754c"
		var got []protocol.SearchMatch
		count, err := scanner.scan(t.Context(), strings.NewReader(input), func(line []byte, number uint64, matched, truncated bool) {
			got = append(got, protocol.SearchMatch{LineNumber: number, Line: string(line), Matched: matched, Truncated: truncated})
		})
		if err != nil || count != 1 || !reflect.DeepEqual(got, want) {
			t.Fatalf("split %d: %d matches, %d rows, error %v; want one match and its context", split, count, len(got), err)
		}
	}
}

func TestLiteralScanValidatesLateUTF8AfterMatch(t *testing.T) {
	t.Parallel()
	scanner := newLiteralSearch(protocol.FileSearch{Query: "needle", Mode: protocol.SearchModeFiles})
	prefix := "NEEDLE\n" + strings.Repeat("text\n", boundedReadChunkSize/4)
	for _, tc := range []struct {
		name, tail string
		want       error
	}{
		{"valid UTF-8", "\u754c\ufffd", nil},
		{"binary after UTF-8", "\u754c\n\x00\n", core.ErrBinaryText},
		{"GB18030", "\xd6\xd0\xce\xc4", errLiteralFallback},
		{"ambiguous C1", "\u0085", errLiteralFallback},
	} {
		t.Run(tc.name, func(t *testing.T) {
			count, err := scanner.scan(t.Context(), strings.NewReader(prefix+tc.tail), nil)
			var wantCount uint64
			if tc.want == nil {
				wantCount = 1
			}
			if count != wantCount || !errors.Is(err, tc.want) {
				t.Fatalf("scan: %d matches, error %v; want %d matches, error %v", count, err, wantCount, tc.want)
			}
		})
	}
}

func BenchmarkLiteralSearchLateUTF8(b *testing.B) {
	prefix := strings.Repeat("mostly ASCII text\n", boundedReadChunkSize/4) + "before\n"
	for _, tc := range []struct{ name, tail string }{
		{"ASCII", "KS\nafter\n"},
		{"UTF8", "\u212a\u017f\nafter\n"},
	} {
		b.Run(tc.name, func(b *testing.B) {
			root, files := filesFixture(b)
			text := prefix + tc.tail
			if err := os.WriteFile(filepath.Join(root, "sample.txt"), []byte(text), 0o600); err != nil {
				b.Fatal(err)
			}
			input := protocol.FileSearch{Query: "ks", Mode: protocol.SearchModeContent, Context: 1, Limit: 3}
			ctx := b.Context()
			b.SetBytes(int64(len(text)))
			b.ReportAllocs()
			for b.Loop() {
				out, err := Search(ctx, files, input)
				if err != nil || len(out.Matches) != 3 || !out.Matches[1].Matched || out.Truncated || out.Skipped != 0 {
					b.Fatalf("search returned %d rows, skipped %d, truncated=%t: %v", len(out.Matches), out.Skipped, out.Truncated, err)
				}
			}
		})
	}
}

func TestLiteralScanReusesMemory(t *testing.T) {
	input := protocol.FileSearch{Query: "needle", Mode: protocol.SearchModeContent, Context: 2}
	scanner := newLiteralSearch(input)
	data := []byte("first\nsecond\n" + strings.Repeat("X", 2*boundedReadChunkSize-2) + "NEEDLE\nafter\n")
	var reader bytes.Reader
	var rows int
	emit := func([]byte, uint64, bool, bool) { rows++ }
	ctx := t.Context()
	allocs := testing.AllocsPerRun(5, func() {
		reader.Reset(data)
		rows = 0
		count, err := scanner.scan(ctx, &reader, emit)
		if err != nil || count != 1 || rows != 4 {
			t.Fatalf("scan got %d matches and %d rows: %v", count, rows, err)
		}
	})
	if allocs != 0 {
		t.Fatalf("warmed scan allocates %g objects per file, want zero", allocs)
	}
	ctx, cancel := context.WithCancel(ctx)
	cancel()
	reader.Reset(data)
	if _, err := scanner.scan(ctx, &reader, emit); !errors.Is(err, context.Canceled) {
		t.Fatalf("canceled scan: %v", err)
	}
}
