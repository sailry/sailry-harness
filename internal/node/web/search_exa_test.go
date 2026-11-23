package web

import "testing"

func TestFormattedExaKeepsMarkdownRulesInContent(t *testing.T) {
	text := "Title: First\nURL: https://example.test/a\nPublished: N/A\nAuthor: N/A\nHighlights:\nintro\n\n---\n\n## read\n\nHead truncation\n\n---\n\n" +
		"Title: Second\nURL: https://example.test/b\nPublished: 2025-01-02\nHighlights:\nbody"
	results, err := formattedExa(text)
	if err != nil {
		t.Fatal(err)
	}
	if len(results) != 2 || results[0].Url != "https://example.test/a" || results[1].Title != "Second" {
		t.Fatalf("results = %+v", results)
	}
	if want := "intro\n\n---\n\n## read\n\nHead truncation"; results[0].Snippet != want {
		t.Fatalf("first snippet = %q, want %q", results[0].Snippet, want)
	}
}
