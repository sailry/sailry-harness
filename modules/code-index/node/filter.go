//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"path"
	"strings"

	"github.com/odvcencio/gotreesitter/grammars"
)

// IndexedSource reports whether a path can contribute declarations and
// references. Unknown names still get a shebang check when discovered.
func IndexedSource(file string) bool {
	_, allowed := indexedLanguage(file)
	return allowed
}

func indexedLanguage(file string) (*grammars.LangEntry, bool) {
	if excludedCodePath(file) {
		return nil, false
	}
	entry := grammars.DetectLanguage(file)
	return entry, entry == nil || sourceLanguage(entry.Name)
}

func indexedCandidate(file string) bool {
	entry, allowed := indexedLanguage(file)
	return allowed && (entry != nil || path.Ext(file) == "")
}

func excludedCodePath(file string) bool {
	file = strings.ToLower(file)
	name := path.Base(file)
	switch name {
	case "go.sum", "go.work.sum", "bun.lockb", "npm-shrinkwrap.json", "lockb":
		return true
	}
	if strings.HasSuffix(name, ".lock") || strings.Contains(name, ".min.") {
		return true
	}
	for directory := path.Dir(file); directory != "." && directory != "/"; directory = path.Dir(directory) {
		switch path.Base(directory) {
		case "vendor", "vendors", "vendored", "third_party", "third-party", "thirdparty", "node_modules", "bower_components", "locale", "locales", "l10n", "translations":
			return true
		}
	}
	return false
}

func sourceLanguage(language string) bool {
	switch language {
	case "json", "json5", "yaml", "toml", "properties", "hcl", "kdl", "cue", "ini", "editorconfig", "csv", "nginx", "caddy", "xml", "dtd", "gomod", "gosum", "gowork", "markdown", "markdown_inline":
		return false
	}
	return true
}

// Minified script and style bundles sometimes have ordinary file names. Limit
// the check to large files mostly made of exceptionally long lines, so a long
// string in otherwise formatted source does not usually mask its declarations.
func minifiedSource(language, source string) bool {
	switch language {
	case "javascript", "jsx", "typescript", "tsx", "css", "scss":
	default:
		return false
	}
	const minLineBytes = 16 << 10
	total := len(source)
	if total < minLineBytes {
		return false
	}
	longBytes := 0
	for source != "" {
		line, rest, _ := strings.Cut(source, "\n")
		if len(line) >= minLineBytes {
			longBytes += len(line)
		}
		source = rest
	}
	return longBytes >= total-total/4
}

// ParseIndexed extracts code only, returning file metadata for excluded paths.
func (p *Parser) ParseIndexed(ctx context.Context, file, source string) (Parsed, error) {
	return p.parseSource(ctx, file, parseInput{text: source}, sourceContent)
}

// ParseOutline returns the same indexed declarations without collecting graph
// references, which an outline never reads.
func (p *Parser) ParseOutline(ctx context.Context, file, source string) (Parsed, error) {
	return p.parseSource(ctx, file, parseInput{text: source}, outlineContent)
}
