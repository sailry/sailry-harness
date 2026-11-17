package kit

import (
	"net/url"
	"strings"

	htmltomarkdown "github.com/JohannesKaufmann/html-to-markdown/v2"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/egoist/mygo/ui/markdown/parser"
	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
)

type documentImageRun struct {
	markdown string
	link     string
}

// Pinned MyGo only loads images in paragraphs containing images alone; raw
// inline <img> and linked images otherwise show as text. Split only those
// document paragraphs into native Markdown runs. The outer view retains the
// original source, and nested views share its selection and image loader.
func documentImageBlock(o *markdown.Options) func(*ui.Context, *parser.Document, int) bool {
	inner := &markdown.Options{}
	var source string
	cache := map[int][]documentImageRun{}
	return func(c *ui.Context, d *parser.Document, i int) bool {
		if o.LoadImage == nil || d.Block(i).Kind != parser.Paragraph {
			return false
		}
		if source != d.Source() {
			source = d.Source()
			clear(cache)
		}
		runs, ok := cache[i]
		if !ok {
			runs = paragraphImageRuns(d, i)
			cache[i] = runs
		}
		if len(runs) == 0 {
			return false
		}
		copyNestedOptions(c, inner, o)
		ui.Column(c).Gap(8).Children(func() {
			for k, run := range runs {
				if run.link == "" {
					markdown.View(c.Key(k), run.markdown, inner)
					continue
				}
				link := ui.Link(c.Key(k), "", "").Tooltip(run.link)
				if link.Clicked() {
					openDocumentLink(c, o, run.link)
				}
				if o.LinkMenu != nil {
					link.ContextMenu(func(m *ui.Menu) { o.LinkMenu(m, run.link) })
				}
				link.Children(func() { markdown.View(c, run.markdown, inner) })
				if o.Preview != nil {
					if fill := o.Preview(c, run.link); fill != nil {
						ui.TooltipBase(c, link, fill)
					}
				}
			}
		})
		return true
	}
}

// Nested runs have no document headings of their own. Route their links through
// the captured outer options so fragment jumps reach the complete document.
func openDocumentLink(c *ui.Context, outer *markdown.Options, dest string) {
	if outer.OnLink != nil {
		outer.OnLink(dest)
		return
	}
	if strings.HasPrefix(dest, "#") {
		outer.Anchor = dest
		outer.AnchorVersion++
		return
	}
	if ui.SafeToOpen(dest) {
		c.OpenURL(dest)
		return
	}
	if router := c.Router(); router != nil {
		u, err := url.Parse(dest)
		if err == nil && !u.IsAbs() && u.Host == "" && !strings.ContainsAny(dest, "\\\x00") && !strings.HasPrefix(dest, "//") {
			router.Push(dest)
		}
	}
}

// paragraphImageRuns uses already resolved link and image destinations, so a
// reference definition elsewhere in the document is not lost when runs split.
func paragraphImageRuns(d *parser.Document, i int) []documentImageRun {
	if d.Block(i).Kind != parser.Paragraph {
		return nil
	}
	in := d.Inlines(i)
	image := false
	for _, n := range in {
		if n.Kind == parser.FootnoteRef {
			// Footnote numbering belongs to the complete native view.
			return nil
		}
		if n.Kind == parser.Image || n.Kind == parser.HTML && strings.Contains(strings.ToLower(n.Text), "<img") {
			image = true
		}
	}
	if !image {
		return nil
	}
	var source strings.Builder
	var write func(int, int)
	write = func(from, to int) {
		for k := from; k < to; k = in[k].Next {
			n := in[k]
			var tag string
			switch n.Kind {
			case parser.HTML:
				source.WriteString(n.Text)
			case parser.Image:
				node := &html.Node{Type: html.ElementNode, Data: "img", DataAtom: atom.Img, Attr: []html.Attribute{
					{Key: "src", Val: n.Dest}, {Key: "alt", Val: parser.PlainText(in[k+1 : n.Next])}, {Key: "title", Val: n.Title},
				}}
				_ = html.Render(&source, node)
			case parser.Link:
				node := &html.Node{Type: html.ElementNode, Data: "a", DataAtom: atom.A, Attr: []html.Attribute{{Key: "href", Val: n.Dest}}}
				var opening strings.Builder
				_ = html.Render(&opening, node)
				source.WriteString(strings.TrimSuffix(opening.String(), "</a>"))
				write(k+1, n.Next)
				source.WriteString("</a>")
			case parser.Emphasis:
				tag = "em"
			case parser.Strong:
				tag = "strong"
			case parser.Strikethrough:
				tag = "del"
			case parser.Code:
				node := &html.Node{Type: html.ElementNode, Data: "code", DataAtom: atom.Code}
				node.AppendChild(&html.Node{Type: html.TextNode, Data: n.Text})
				_ = html.Render(&source, node)
			case parser.SoftBreak:
				source.WriteByte('\n')
			case parser.LineBreak:
				source.WriteString("<br>")
			default:
				text := n.Text
				if n.Kind == parser.Math {
					text = d.Source()[n.Start:n.End]
				}
				_ = html.Render(&source, &html.Node{Type: html.TextNode, Data: text})
			}
			if tag != "" {
				source.WriteString("<" + tag + ">")
				write(k+1, n.Next)
				source.WriteString("</" + tag + ">")
			}
		}
	}
	write(0, len(in))
	return splitDocumentImages(source.String())
}

// splitDocumentImages retains ancestor formatting on either side of an image.
// Cloning only the current ancestors keeps a surrounding link on both text
// runs and records its destination for the image's native link.
func splitDocumentImages(source string) []documentImageRun {
	body := &html.Node{Type: html.ElementNode, Data: "body", DataAtom: atom.Body}
	nodes, err := html.ParseFragment(strings.NewReader(source), body)
	if err != nil {
		return nil
	}
	var runs []documentImageRun
	image := false
	root := &html.Node{Type: html.ElementNode, Data: "div", DataAtom: atom.Div}
	parents := map[*html.Node]*html.Node{}
	flush := func() {
		md, err := htmltomarkdown.ConvertNode(root)
		if err == nil && strings.TrimSpace(string(md)) != "" {
			runs = append(runs, documentImageRun{markdown: string(md)})
		}
		root = &html.Node{Type: html.ElementNode, Data: "div", DataAtom: atom.Div}
		clear(parents)
	}
	var visit func(*html.Node, []*html.Node)
	visit = func(n *html.Node, ancestors []*html.Node) {
		if n.Type == html.ElementNode && n.DataAtom == atom.Img {
			image = true
			flush()
			md, err := htmltomarkdown.ConvertNode(n)
			if err != nil || strings.TrimSpace(string(md)) == "" {
				return
			}
			run := documentImageRun{markdown: string(md)}
			for _, parent := range ancestors {
				if parent.DataAtom == atom.A {
					for _, attr := range parent.Attr {
						if attr.Key == "href" {
							run.link = attr.Val
						}
					}
				}
			}
			runs = append(runs, run)
			return
		}
		if n.FirstChild != nil {
			for child := n.FirstChild; child != nil; child = child.NextSibling {
				visit(child, append(ancestors, n))
			}
			return
		}
		parent := root
		for _, ancestor := range ancestors {
			copy := parents[ancestor]
			if copy == nil {
				copy = &html.Node{Type: ancestor.Type, Data: ancestor.Data, DataAtom: ancestor.DataAtom, Attr: ancestor.Attr}
				parent.AppendChild(copy)
				parents[ancestor] = copy
			}
			parent = copy
		}
		parent.AppendChild(&html.Node{Type: n.Type, Data: n.Data, DataAtom: n.DataAtom, Attr: n.Attr})
	}
	for _, n := range nodes {
		visit(n, nil)
	}
	flush()
	if !image {
		return nil
	}
	return runs
}
