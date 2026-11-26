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

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLiteralSearchPreservesResults(t *testing.T) {
	t.Parallel()
	root, files := filesFixture(t)
	for name, content := range map[string]string{
		".gitignore":     "ignored.txt\n",
		"a-lines.txt":    "\xef\xbb\xbfzero\r\none\r\ntwo\nNeed.le\r\nbetween\nneed.le and need.le\nend\r",
		"b-binary.txt":   "need.le\n" + strings.Repeat("text\n", boundedReadChunkSize/4) + "\x01\n",
		"c-gb.txt":       "need.le\n" + strings.Repeat("text\n", boundedReadChunkSize/4) + "\xd6\xd0\xce\xc4\n",
		"d-boundary.txt": "before\n" + strings.Repeat("x", boundedReadChunkSize-3) + "NEED.LE" + strings.Repeat("y", 2*boundedReadChunkSize) + "\r\nafter",
		"e-utf8.txt":     strings.Repeat("x", searchLineByteLimit-1) + "\u754c" + strings.Repeat("x", boundedReadChunkSize-searchLineByteLimit-4) + "\u5339\u914d\n",
		"f-fold.txt":     "before\n\u212a and \u017f\nend\n",
		"g-control.txt":  "need.le\n\u0085\n",
		"h-cr.txt":       strings.Repeat("x", boundedReadChunkSize-1) + "\r\nlast\rcarriage\r",
		"ignored.txt":    "need.le\n",
		"excluded.md":    "need.le\n",
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
