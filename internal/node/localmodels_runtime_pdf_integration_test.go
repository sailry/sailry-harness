//go:build integration

package node

import (
	"os"
	"path/filepath"
	"testing"
)

// TestScannedInspection reads PDF files through the Node: pages without a
// text layer are rasterized and recognized, pages with text are untouched.
func TestScannedInspection(t *testing.T) {
	node, fixtures := ocrNode(t)
	for _, name := range []string{"scan", "vector"} {
		t.Run(name, func(t *testing.T) {
			pages := recordedPages(t, filepath.Join(fixtures, "rust-pdf-"+name+".json"))
			data, err := os.ReadFile(filepath.Join(fixtures, "ocr", name+".pdf"))
			if err != nil {
				t.Fatal(err)
			}
			inspection, err := node.inspectDocument(t.Context(), name+".pdf", data, 0)
			if err != nil {
				t.Fatal(err)
			}
			if len(inspection.Sections) != len(pages) {
				t.Fatalf("%d sections, want %d: %#v", len(inspection.Sections), len(pages), inspection.Sections)
			}
			for _, section := range inspection.Sections {
				t.Logf("%s: %q", section.Name, section.Text)
			}
			if name == "vector" {
				// The page has a text layer, so it is not recognized.
				return
			}
			for index, section := range inspection.Sections {
				if section.Text != pages[index] {
					t.Errorf("%s = %q, want %q", section.Name, section.Text, pages[index])
				}
			}
		})
	}
}
