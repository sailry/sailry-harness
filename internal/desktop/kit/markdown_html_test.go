package kit

import (
	"strings"
	"testing"
)

// An HTML block splits into runs, centred or not, each as Markdown: links
// stay links and images retain their sources and descriptions.
func TestHTMLChunks(t *testing.T) {
	chunks := splitHTML(`<p align="center">
  <a href="docs/hosts.md">Hosts</a> ·
  <a href="https://example.com"><img src="badge.svg" alt="Badge"></a>
</p>
<details><summary>More</summary>Text</details>
<center><b>Bold</b></center>`)
	if len(chunks) != 3 || !chunks[0].centred || chunks[1].centred || !chunks[2].centred {
		t.Fatalf("chunks %+v", chunks)
	}
	if !strings.Contains(chunks[0].markdown, "[Hosts](docs/hosts.md)") || !strings.Contains(chunks[0].markdown, "Badge") {
		t.Errorf("centred chunk %q", chunks[0].markdown)
	}
	if !strings.Contains(chunks[2].markdown, "**Bold**") {
		t.Errorf("centre chunk %q", chunks[2].markdown)
	}
}
