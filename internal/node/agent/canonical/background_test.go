package canonical

import (
	"strings"
	"testing"
	"unicode/utf8"
)

func TestChildResponseMarksTruncation(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name  string
		text  string
		total uint64
		cut   bool
	}{
		{"whole", "short answer", 12, false},
		{"cut at a rune boundary", strings.Repeat("é", 10), 20, true},
		{"already cut by the store", "stored prefix", 4000, true},
	} {
		t.Run(row.name, func(t *testing.T) {
			t.Parallel()
			var result BackgroundResult
			result.ChildResponse(row.text, row.total, 13)
			if result.Response == nil || !utf8.ValidString(*result.Response) || len(*result.Response) > 13 {
				t.Fatalf("response = %v", result.Response)
			}
			if result.ResponseTruncated != row.cut || (result.ResponseBytes != nil) != row.cut || (result.ResponseHint != "") != row.cut {
				t.Fatalf("truncation = %+v", result)
			}
			if row.cut && *result.ResponseBytes != row.total {
				t.Fatalf("response bytes = %d, want %d", *result.ResponseBytes, row.total)
			}
		})
	}
}
