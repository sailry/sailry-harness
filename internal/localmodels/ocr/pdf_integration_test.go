//go:build integration

package ocr

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

// TestReferencePDF rasterizes the fixture documents with PDFium, recognizes
// every page and compares the text with the reference, which rasterizes with
// a different renderer. Image-only and embedded-font pages must agree
// exactly. The page typeset by a system print filter is the accepted
// deviation of docs/models.md: it may differ by isolated characters, so its
// lines are compared with their spaces removed.
func TestReferencePDF(t *testing.T) {
	engine, fixtures := integrationEngine(t)
	spaceless := func(text string) string {
		var kept []rune
		for _, value := range text {
			if value != ' ' {
				kept = append(kept, value)
			}
		}
		return string(kept)
	}
	for _, test := range []struct {
		name  string
		exact bool
	}{{"scan", true}, {"vector", true}, {"text", false}} {
		t.Run(test.name, func(t *testing.T) {
			recorded := recordings(t, filepath.Join(fixtures, "rust-pdf-"+test.name+".json"))
			data, err := os.ReadFile(filepath.Join(fixtures, "ocr", test.name+".pdf"))
			if err != nil {
				t.Fatal(err)
			}
			document, err := OpenPDF(context.Background(), data)
			if err != nil {
				t.Fatal(err)
			}
			defer document.Close()
			if document.Pages() != len(recorded) {
				t.Fatalf("%d pages, want %d", document.Pages(), len(recorded))
			}
			for page, want := range recorded {
				image, err := document.Render(page)
				if err != nil {
					t.Fatal(err)
				}
				lines, err := engine.Recognize(image, func() bool { return false })
				if err != nil {
					t.Fatal(err)
				}
				if len(lines) != len(want.Lines) {
					t.Fatalf("page %d: %d lines, want %d: %v", page+1, len(lines), len(want.Lines), lines)
				}
				for index, line := range lines {
					expected := want.Lines[index].Text
					switch {
					case line.Text == expected:
					case !test.exact && spaceless(line.Text) == spaceless(expected):
						t.Logf("page %d line %d differs in spacing: %q, reference %q", page+1, index, line.Text, expected)
					default:
						t.Errorf("page %d line %d = %q, want %q", page+1, index, line.Text, expected)
					}
				}
				t.Logf("page %d: %dx%d pixels, %d lines", page+1, image.W, image.H, len(lines))
			}
		})
	}
}
