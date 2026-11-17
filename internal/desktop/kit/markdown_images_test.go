package kit

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui/markdown/parser"
)

func TestDocumentParagraphImages(t *testing.T) {
	for _, tc := range []struct {
		name, source string
		count        int
		link         string
	}{
		{name: "plain", source: "Text without images."},
		{name: "inline code", source: "Text `<img src=\"secret.png\">` after."},
		{name: "fenced code", source: "```html\n<img src=\"secret.png\">\n```"},
		{name: "inline HTML", source: `Before <img src="../images/logo.svg" alt="Logo"> after.`, count: 3},
		{name: "linked HTML", source: `Before <a href="https://example.test"><img src="../images/logo.svg" alt="Logo"></a> after.`, count: 3, link: "https://example.test"},
		{name: "reference destinations", source: "**Before** ![Logo][image] after [Guide][guide].\n\n[image]: ../images/logo.svg\n[guide]: https://example.test/guide", count: 3},
	} {
		t.Run(tc.name, func(t *testing.T) {
			d := parser.Parse(tc.source)
			runs := paragraphImageRuns(d, 1)
			if len(runs) != tc.count {
				t.Fatalf("runs %+v, want %d", runs, tc.count)
			}
			if d.Source() != tc.source {
				t.Fatal("the document source changed")
			}
			if len(runs) == 0 {
				return
			}
			image := parser.Parse(runs[1].markdown).Inlines(1)
			if len(image) == 0 || image[0].Kind != parser.Image || image[0].Dest != "../images/logo.svg" {
				t.Fatalf("image run %q did not preserve the resolved source", runs[1].markdown)
			}
			if runs[1].link != tc.link {
				t.Fatalf("image link %q, want %q", runs[1].link, tc.link)
			}
			if tc.name == "reference destinations" && (!strings.Contains(runs[0].markdown, "**Before**") || !strings.Contains(runs[2].markdown, "https://example.test/guide")) {
				t.Fatalf("surrounding formatting or resolved link lost: %+v", runs)
			}
		})
	}
}

func TestDocumentImageHTMLAncestors(t *testing.T) {
	runs := splitDocumentImages(`<strong>Before <a href="https://example.test">linked <img src="logo.png" alt="Logo"> text</a> after</strong>`)
	if len(runs) != 3 || runs[1].link != "https://example.test" {
		t.Fatalf("image and link runs %+v", runs)
	}
	if !strings.Contains(runs[0].markdown, "**Before") || !strings.Contains(runs[0].markdown, "https://example.test") || !strings.Contains(runs[2].markdown, "https://example.test") || !strings.Contains(runs[2].markdown, "after**") {
		t.Fatalf("ancestor formatting or surrounding links lost: %+v", runs)
	}
}
