package store

import (
	"io/fs"
	"net/url"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/text"
)

func conversationAssetMarkdown(targets *conversationAssetTargets, markdown string, root *string, input bool) {
	source := []byte(markdown)
	document := goldmark.New().Parser().Parse(text.NewReader(source))
	paragraph, standalone := false, false
	quotes, links, linkDepth := 0, 0, 0
	delivery := ""
	link := func(destination string) {
		if input {
			if path, ok := conversationAssetPath(destination, root); ok {
				targets.file(path)
			}
		} else if paragraph {
			links++
			if links > 1 {
				standalone = false
			}
			delivery = ""
			if !strings.ContainsAny(destination, "#?") && !assetSourceLine(destination) {
				delivery, _ = conversationAssetPath(destination, root)
			}
		}
	}
	_ = ast.Walk(document, func(node ast.Node, entering bool) (ast.WalkStatus, error) {
		switch value := node.(type) {
		case *ast.Blockquote:
			if entering {
				quotes++
			} else {
				quotes--
			}
		case *ast.Paragraph, *ast.TextBlock, *ast.ListItem:
			if entering {
				paragraph, standalone, delivery, links = true, quotes == 0, "", 0
			} else {
				if standalone && delivery != "" {
					targets.file(delivery)
				}
				paragraph, delivery = false, ""
			}
		case *ast.Link:
			if entering {
				linkDepth++
				link(string(value.Destination))
			} else {
				linkDepth--
			}
		case *ast.AutoLink:
			if entering {
				link(string(value.URL(source)))
			}
		case *ast.Image:
			if entering {
				standalone = false
				if path, ok := conversationAssetPath(string(value.Destination), root); ok {
					targets.file(path)
				}
			}
		case *ast.Text:
			if entering && linkDepth == 0 && len(strings.TrimSpace(string(value.Segment.Value(source)))) > 0 {
				standalone = false
			}
		case *ast.String:
			if entering && linkDepth == 0 && len(strings.TrimSpace(string(value.Value))) > 0 {
				standalone = false
			}
		case *ast.RawHTML, *ast.HTMLBlock:
			standalone = false
		}
		return ast.WalkContinue, nil
	})
}

func assetSourceLine(link string) bool {
	colon := strings.LastIndexByte(link, ':')
	if colon < 0 || colon == len(link)-1 {
		return false
	}
	for _, value := range link[colon+1:] {
		if value < '0' || value > '9' {
			return false
		}
	}
	return true
}
func assetNodePath(path string) string {
	path = strings.ReplaceAll(path, "\\", "/")
	if rest, ok := strings.CutPrefix(path, "//?/UNC/"); ok {
		return "//" + rest
	}
	return strings.TrimPrefix(path, "//?/")
}
func assetURLPath(path string) string {
	if !strings.HasPrefix(path, "/") {
		path = "/" + path
	}
	if len(path) >= 3 && path[1] >= 'a' && path[1] <= 'z' && path[2] == ':' && (len(path) == 3 || path[3] == '/') {
		path = path[:1] + strings.ToUpper(path[1:2]) + path[2:]
	}
	return path
}
func validAssetRelative(path string) bool {
	return utf8.ValidString(path) && fs.ValidPath(path) && path != "." && !strings.ContainsAny(path, "\\:") && !strings.ContainsFunc(path, unicode.IsControl)
}
func conversationAssetPath(link string, root *string) (string, bool) {
	if link == "" || strings.HasPrefix(link, "#") || root == nil && (strings.HasPrefix(link, "/") || strings.ContainsAny(link, ":\\")) {
		return "", false
	}
	basePath := "/"
	if root != nil {
		basePath = assetNodePath(*root)
	}
	base := &url.URL{Scheme: "file", Path: strings.TrimRight(assetURLPath(basePath), "/") + "/"}
	link = assetNodePath(link)
	if end := strings.IndexAny(link, "#?"); end >= 0 {
		link = link[:end]
	}
	if assetSourceLine(link) {
		link = link[:strings.LastIndexByte(link, ':')]
	}
	if root == nil {
		decoded, err := url.PathUnescape(link)
		for strings.HasPrefix(decoded, "./") {
			decoded = strings.TrimPrefix(decoded, "./")
		}
		if err != nil || !validAssetRelative(decoded) {
			return "", false
		}
	}
	var resolved *url.URL
	if len(link) > 1 && link[1] == ':' || strings.HasPrefix(link, "//") {
		resolved = &url.URL{Scheme: "file", Path: assetURLPath(link)}
	} else {
		reference, err := url.Parse(link)
		if err != nil {
			return "", false
		}
		resolved = base.ResolveReference(reference)
		resolved.Path = assetURLPath(resolved.Path)
	}
	if resolved.Scheme != base.Scheme || resolved.Host != base.Host || resolved.User != nil || !strings.HasPrefix(resolved.Path, base.Path) {
		return "", false
	}
	relative := strings.ReplaceAll(strings.TrimPrefix(resolved.Path, base.Path), "\\", "/")
	return relative, validAssetRelative(relative)
}
