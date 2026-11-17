package fileview

import (
	"net/url"
	"path"
	"strings"
	"unicode"
)

// ResolveImagePath resolves a document image inside its captured worktree.
// URL queries and fragments do not change the file being downloaded.
func ResolveImagePath(document, reference string) (string, bool) {
	valid := func(value string) bool {
		return value != "" && !strings.ContainsAny(value, "\\:") && !strings.HasPrefix(value, "/") &&
			!strings.ContainsFunc(value, unicode.IsControl)
	}
	if !valid(document) || path.Clean(document) != document || document == "." || document == ".." || strings.HasPrefix(document, "../") {
		return "", false
	}
	uri, err := url.Parse(reference)
	if err != nil || uri.Scheme != "" || uri.Host != "" || uri.User != nil || uri.Opaque != "" || !valid(uri.Path) {
		return "", false
	}
	resolved := path.Join(path.Dir(document), uri.Path)
	if resolved == "." || resolved == ".." || strings.HasPrefix(resolved, "../") {
		return "", false
	}
	return resolved, true
}
