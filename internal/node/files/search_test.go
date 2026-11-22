package files

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSearchScansMultipleFiles(t *testing.T) {
	t.Parallel()
	root, files := filesFixture(t)
	longPrefix := strings.Repeat("x", (16<<10)-1)
	for name, content := range map[string]string{
		".gitignore":      "d-ignored.txt\ne-override.txt\n",
		".ignore":         "!e-override.txt\n",
		"a.txt":           "\xef\xbb\xbfbefore A\r\nNEEDLE alpha\r\nafter A\r\n",
		"b-binary.txt":    "needle\x00\n",
		"c-long.txt":      "before C\n" + longPrefix + "\u754c" + strings.Repeat("x", 64<<10) + " needle charlie\nafter C",
		"d-ignored.txt":   "needle ignored\n",
		"e-override.txt":  "before E\nneedle \xd6\xd0\xce\xc4\nafter E\n",
		"f-oversized.txt": "",
		"g-skipped.md":    "needle wrong extension\n",
		"h-skipped.txt":   "needle excluded by glob\n",
	} {
		if err := os.WriteFile(filepath.Join(root, name), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Truncate(filepath.Join(root, "f-oversized.txt"), (64<<20)+1); err != nil {
		t.Fatal(err)
	}

	result, err := Search(t.Context(), files, protocol.FileSearch{
		Query: "needle", Globs: []string{"*.txt", "!h-skipped.txt"}, Mode: protocol.SearchModeContent, Context: 1, Limit: 20,
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.ScannedFiles != 5 || result.Skipped != 2 || result.Partial || !result.Truncated || result.NextOffset != nil {
		t.Errorf("search scanned %d, skipped %d, partial=%t, truncated=%t, next=%v", result.ScannedFiles, result.Skipped, result.Partial, result.Truncated, result.NextOffset)
	}
	if len(result.Diagnostics) != 1 || result.Diagnostics[0].Path != "f-oversized.txt" || !strings.Contains(result.Diagnostics[0].Message, "64 MiB search limit") {
		t.Errorf("search diagnostics = %+v", result.Diagnostics)
	}
	want := []protocol.SearchMatch{
		{Path: "a.txt", LineNumber: 1, Line: "before A"},
		{Path: "a.txt", LineNumber: 2, Line: "NEEDLE alpha", Matched: true},
		{Path: "a.txt", LineNumber: 3, Line: "after A"},
		{Path: "c-long.txt", LineNumber: 1, Line: "before C"},
		{Path: "c-long.txt", LineNumber: 2, Line: longPrefix, Matched: true, Truncated: true},
		{Path: "c-long.txt", LineNumber: 3, Line: "after C"},
		{Path: "e-override.txt", LineNumber: 1, Line: "before E"},
		{Path: "e-override.txt", LineNumber: 2, Line: "needle \u4e2d\u6587", Matched: true},
		{Path: "e-override.txt", LineNumber: 3, Line: "after E"},
	}
	if len(result.Matches) != len(want) {
		t.Fatalf("search returned %d rows, want %d", len(result.Matches), len(want))
	}
	for i, expected := range want {
		got := result.Matches[i]
		if got.Line != expected.Line {
			t.Errorf("row %d text = %.64q (%d bytes), want %.64q (%d bytes)", i, got.Line, len(got.Line), expected.Line, len(expected.Line))
		}
		got.Line, expected.Line = "", ""
		if got != expected {
			t.Errorf("row %d metadata = %+v, want %+v", i, got, expected)
		}
	}
}

func BenchmarkSearch(b *testing.B) {
	const fileCount = 64
	for _, fixture := range []struct {
		name       string
		largeEvery int
	}{{"small", 0}, {"mixed", 8}} {
		b.Run(fixture.name, func(b *testing.B) {
			root, files := filesFixture(b)
			const filler = "the quick brown fox jumps over the lazy dog\n"
			small, large := strings.Repeat(filler, 48), strings.Repeat(filler, 3072)
			var total int64
			for i := range fileCount {
				name := fmt.Sprintf("file-%02d.txt", i)
				body := small
				if fixture.largeEvery > 0 && i%fixture.largeEvery == 0 {
					body = large
				}
				text := body + "before\nneedle " + name + "\nafter\n"
				if err := os.WriteFile(filepath.Join(root, name), []byte(text), 0o600); err != nil {
					b.Fatal(err)
				}
				total += int64(len(text))
			}
			options := protocol.FileSearch{Query: "needle", Mode: protocol.SearchModeContent, Context: 1, Limit: 3 * fileCount}
			ctx := b.Context()
			b.ReportAllocs()
			b.SetBytes(total)
			for b.Loop() {
				out, err := Search(ctx, files, options)
				if err != nil || out.ScannedFiles != fileCount || out.Skipped != 0 || len(out.Matches) != 3*fileCount || out.Truncated {
					b.Fatalf("search scanned %d, skipped %d, returned %d rows, truncated=%t (%v)", out.ScannedFiles, out.Skipped, len(out.Matches), out.Truncated, err)
				}
			}
		})
	}
}
