package kit

import (
	"strings"
	"sync"

	htmltomarkdown "github.com/JohannesKaufmann/html-to-markdown/v2"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
)

// MyGo's markdown view leaves HTML blocks to its Options.HTML and shows
// them as their source without one; it has no HTML renderer of its own.
// A document's blocks, as a README's centred badges and links, show here
// as the Markdown they convert to, in a view sharing the document's look,
// links and selection. A block aligned to the centre (align="center", or
// <center>) shows centred. Images show as their descriptions, as the
// document's own do while no LoadImage is given.

// htmlChunk is a part of an HTML block: its Markdown, and whether it is
// centred.
type htmlChunk struct {
	markdown string
	centred  bool
}

var (
	htmlChunksMu sync.Mutex
	htmlChunks   = map[string][]htmlChunk{} // by the block's HTML
	nestedMu     sync.Mutex
	nested       = map[*markdown.Options]*markdown.Options{}
)

// htmlBlock builds an HTML block of the document o shows.
func htmlBlock(o *markdown.Options) func(c *ui.Context, source string) {
	return func(c *ui.Context, source string) {
		inner := nestedOptions(o)
		ui.Column(c).FillWidth().Children(func() {
			for i, chunk := range chunksOf(source) {
				view := markdown.View(c.Key(i), chunk.markdown, inner)
				if chunk.centred {
					view.AlignSelf(ui.Center)
				}
			}
		})
	}
}

// nestedOptions are the Options of the views within o's document: its look
// and links, sharing its selection. They take no HTML of their own, so a
// tag the conversion leaves shows as text.
func nestedOptions(o *markdown.Options) *markdown.Options {
	nestedMu.Lock()
	defer nestedMu.Unlock()
	inner := nested[o]
	if inner == nil {
		inner = &markdown.Options{}
		nested[o] = inner
	}
	inner.Theme, inner.Extensions, inner.CodeBlock = o.Theme, o.Extensions, o.CodeBlock
	inner.LinkMenu, inner.OnLink, inner.Preview, inner.LoadImage = o.LinkMenu, o.OnLink, o.Preview, o.LoadImage
	inner.Reveal, inner.ShareSelection = o.Reveal, true
	return inner
}

// chunksOf splits an HTML block into runs of its top-level nodes, centred
// or not, each converted to Markdown once.
func chunksOf(source string) []htmlChunk {
	htmlChunksMu.Lock()
	defer htmlChunksMu.Unlock()
	if chunks, ok := htmlChunks[source]; ok {
		return chunks
	}
	if len(htmlChunks) > 256 {
		clear(htmlChunks)
	}
	chunks := splitHTML(source)
	htmlChunks[source] = chunks
	return chunks
}

func splitHTML(source string) []htmlChunk {
	body := &html.Node{Type: html.ElementNode, Data: "body", DataAtom: atom.Body}
	nodes, err := html.ParseFragment(strings.NewReader(source), body)
	if err != nil {
		return []htmlChunk{{markdown: source}}
	}
	var chunks []htmlChunk
	var run strings.Builder
	centred := false
	flush := func() {
		if run.Len() == 0 {
			return
		}
		md, err := htmltomarkdown.ConvertString(run.String())
		run.Reset()
		if err != nil || strings.TrimSpace(md) == "" {
			return
		}
		chunks = append(chunks, htmlChunk{markdown: md, centred: centred})
	}
	for _, n := range nodes {
		if n.Type == html.TextNode && strings.TrimSpace(n.Data) == "" {
			continue
		}
		if c := isCentred(n); c != centred {
			flush()
			centred = c
		}
		if err := html.Render(&run, n); err != nil {
			return []htmlChunk{{markdown: source}}
		}
	}
	flush()
	return chunks
}

// isCentred reports whether n is a block aligned to the centre.
func isCentred(n *html.Node) bool {
	if n.Type != html.ElementNode {
		return false
	}
	if n.DataAtom == atom.Center {
		return true
	}
	for _, a := range n.Attr {
		if a.Key == "align" && strings.EqualFold(strings.TrimSpace(a.Val), "center") {
			return true
		}
	}
	return false
}
