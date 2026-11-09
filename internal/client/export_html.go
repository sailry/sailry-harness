package client

import (
	"bytes"
	"html"
	"strings"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/ast"
	"github.com/yuin/goldmark/extension"
	"github.com/yuin/goldmark/parser"
	"github.com/yuin/goldmark/renderer"
	"github.com/yuin/goldmark/text"
	"github.com/yuin/goldmark/util"
)

var exportMathKind = ast.NewNodeKind("ExportMath")

type exportMath struct {
	ast.BaseInline
	value   string
	display bool
}

func (n *exportMath) Kind() ast.NodeKind            { return exportMathKind }
func (n *exportMath) Dump(source []byte, level int) { ast.DumpHelper(n, source, level, nil, nil) }

type exportMathParser struct{}

func (exportMathParser) Trigger() []byte { return []byte{'$', '\\'} }
func (exportMathParser) Parse(parent ast.Node, reader text.Reader, _ parser.Context) ast.Node {
	line, _ := reader.PeekLine()
	if len(line) == 0 || reader.PrecendingCharacter() == '\\' {
		return nil
	}
	opener, closer := "$", "$"
	display := false
	if bytes.HasPrefix(line, []byte("$$")) {
		opener, closer = "$$", "$$"
		display = true
	} else if bytes.HasPrefix(line, []byte(`\(`)) {
		opener, closer = `\(`, `\)`
	} else if bytes.HasPrefix(line, []byte(`\[`)) {
		opener, closer = `\[`, `\]`
		display = true
	} else if line[0] != '$' {
		return nil
	}
	savedLine, savedPosition := reader.Position()
	reader.Advance(len(opener))
	var out strings.Builder
	for {
		line, _ = reader.PeekLine()
		if line == nil {
			reader.SetPosition(savedLine, savedPosition)
			return nil
		}
		index := bytes.Index(line, []byte(closer))
		for index >= 0 && index > 0 && line[index-1] == '\\' {
			next := bytes.Index(line[index+1:], []byte(closer))
			if next < 0 {
				index = -1
			} else {
				index += next + 1
			}
		}
		if index >= 0 {
			out.Write(line[:index])
			if strings.TrimSpace(out.String()) == "" {
				reader.SetPosition(savedLine, savedPosition)
				return nil
			}
			reader.Advance(index + len(closer))
			return &exportMath{value: out.String(), display: display}
		}
		out.Write(line)
		reader.AdvanceLine()
	}
}

type exportHTMLRenderer struct{}

func (exportHTMLRenderer) RegisterFuncs(reg renderer.NodeRendererFuncRegisterer) {
	reg.Register(ast.KindRawHTML, func(w util.BufWriter, source []byte, n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			_, e := w.WriteString(html.EscapeString(string(n.(*ast.RawHTML).Text(source))))
			return ast.WalkContinue, e
		}
		return ast.WalkContinue, nil
	})
	reg.Register(ast.KindHTMLBlock, func(w util.BufWriter, source []byte, n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			_, e := w.WriteString(html.EscapeString(string(n.(*ast.HTMLBlock).Text(source))))
			return ast.WalkContinue, e
		}
		return ast.WalkContinue, nil
	})
	reg.Register(exportMathKind, func(w util.BufWriter, _ []byte, n ast.Node, entering bool) (ast.WalkStatus, error) {
		if entering {
			math := n.(*exportMath)
			open, close := `<span class="math inline">\(`, `\)</span>`
			if math.display {
				open, close = `<div class="math display">\[`, `\]</div>`
			}
			_, e := w.WriteString(open + html.EscapeString(math.value) + close)
			return ast.WalkContinue, e
		}
		return ast.WalkContinue, nil
	})
}
func exportHTMLBody(markdown string) (string, error) {
	processor := goldmark.New(goldmark.WithExtensions(extension.Table, extension.Strikethrough, extension.TaskList), goldmark.WithParserOptions(parser.WithInlineParsers(util.Prioritized(exportMathParser{}, 100))), goldmark.WithRendererOptions(renderer.WithNodeRenderers(util.Prioritized(exportHTMLRenderer{}, 500))))
	source := []byte(markdown)
	document := processor.Parser().Parse(text.NewReader(source))
	_ = ast.Walk(document, func(n ast.Node, entering bool) (ast.WalkStatus, error) {
		if !entering {
			return ast.WalkContinue, nil
		}
		var destination *[]byte
		switch v := n.(type) {
		case *ast.Link:
			destination = &v.Destination
		case *ast.Image:
			destination = &v.Destination
		}
		if destination != nil {
			value := string(*destination)
			if strings.ContainsRune(value, ':') && !safeExportURL(value) {
				*destination = []byte("#")
			}
		}
		return ast.WalkContinue, nil
	})
	var out bytes.Buffer
	if err := processor.Renderer().Render(&out, source, document); err != nil {
		return "", err
	}
	return out.String(), nil
}
