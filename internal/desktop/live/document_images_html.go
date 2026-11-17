package live

import (
	"encoding/base64"
	"strings"

	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
)

// inlineHTML supplies image bytes to the existing opaque-origin sandbox. It
// neither gives the page a file base URL nor relaxes its network policy.
func (d *documentImages) inlineHTML(source string) string {
	tokens := html.NewTokenizer(strings.NewReader(source))
	var result strings.Builder
	for {
		kind := tokens.Next()
		if kind == html.ErrorToken {
			return result.String()
		}
		if kind != html.StartTagToken && kind != html.SelfClosingTagToken {
			result.Write(tokens.Raw())
			continue
		}
		raw := string(tokens.Raw())
		token := tokens.Token()
		if token.DataAtom != atom.Img && token.DataAtom != atom.Source {
			result.WriteString(raw)
			continue
		}
		attributes := token.Attr[:0]
		for _, attribute := range token.Attr {
			if attribute.Key == "srcset" {
				continue
			}
			if token.DataAtom == atom.Img && attribute.Key == "src" {
				image := d.image(attribute.Val)
				if image.err != nil {
					// The browser keeps the original alt text as the placeholder.
					continue
				}
				attribute.Val = "data:" + image.mime + ";base64," + base64.StdEncoding.EncodeToString(image.data)
			}
			attributes = append(attributes, attribute)
		}
		token.Attr = attributes
		result.WriteString(token.String())
	}
}
