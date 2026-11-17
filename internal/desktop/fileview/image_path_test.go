package fileview

import "testing"

func TestResolveDocumentImagePath(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		document, reference, want string
	}{
		{"docs/readme.md", "../images/logo.png", "images/logo.png"},
		{"docs/readme.md", "./logo.svg?v=2#mark", "docs/logo.svg"},
		{"docs/readme.md", "../images/my%20logo.png", "images/my logo.png"},
		{"docs/readme.md", "%2e%2e/images/logo.png", "images/logo.png"},
		{"docs/readme.md", "%252e%252e/logo.png", "docs/%2e%2e/logo.png"},
		{"readme.md", "../../private.png", ""},
		{"docs/readme.md", "../../private.png", ""},
		{"docs/readme.md", "%2fprivate.png", ""},
		{"docs/readme.md", "../images/%5cprivate.png", ""},
		{"docs/readme.md", "../images/a%00.png", ""},
		{"docs/readme.md", "/images/logo.png", ""},
		{"docs/readme.md", "file:../images/logo.png", ""},
		{"docs/readme.md", "https://example.com/logo.png", ""},
		{"docs/readme.md", "//example.com/logo.png", ""},
		{"docs/readme.md", "C:/private.png", ""},
		{"", "logo.png", ""},
		{"../readme.md", "logo.png", ""},
	} {
		t.Run(test.document+"/"+test.reference, func(t *testing.T) {
			got, ok := ResolveImagePath(test.document, test.reference)
			if got != test.want || ok != (test.want != "") {
				t.Fatalf("ResolveImagePath(%q, %q) = %q, %v; want %q", test.document, test.reference, got, ok, test.want)
			}
		})
	}
}
