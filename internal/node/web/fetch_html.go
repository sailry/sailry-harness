package web

import (
	"bytes"
	"io"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/JohannesKaufmann/html-to-markdown/v2/converter"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/base"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/commonmark"
	"github.com/JohannesKaufmann/html-to-markdown/v2/plugin/table"
	"github.com/microcosm-cc/bluemonday"
	"golang.org/x/net/html"
	"golang.org/x/net/html/charset"
	"golang.org/x/text/encoding"
	"golang.org/x/text/encoding/simplifiedchinese"
	unicodeencoding "golang.org/x/text/encoding/unicode"
)

var readableHTML = bluemonday.UGCPolicy()

func convertHTML(value string) (*string, string, error) {
	document, err := html.Parse(strings.NewReader(value))
	if err != nil {
		return nil, "", webUnavailable("web HTML extraction failed")
	}
	var title *string
	var main *html.Node
	var articles []*html.Node
	var text func(*html.Node) string
	text = func(node *html.Node) string {
		if node.Type == html.TextNode {
			return node.Data
		}
		var value strings.Builder
		for child := node.FirstChild; child != nil; child = child.NextSibling {
			value.WriteString(text(child))
		}
		return value.String()
	}
	attribute := func(node *html.Node, key string) (string, bool) {
		for _, attribute := range node.Attr {
			if attribute.Key == key {
				return strings.ToLower(strings.TrimSpace(attribute.Val)), true
			}
		}
		return "", false
	}
	var clean func(*html.Node)
	clean = func(node *html.Node) {
		for child := node.FirstChild; child != nil; {
			next := child.NextSibling
			if child.Type != html.ElementNode {
				clean(child)
				child = next
				continue
			}
			tag := child.Data
			if tag == "title" && title == nil {
				value := strings.Join(strings.Fields(text(child)), " ")
				if value != "" {
					title = &value
				}
			}
			role, _ := attribute(child, "role")
			_, hidden := attribute(child, "hidden")
			aria, _ := attribute(child, "aria-hidden")
			container := strings.Contains("|div|section|header|ul|ol|span|table|p|main|article|", "|"+tag+"|")
			skipped := strings.Contains("|script|style|template|nav|footer|aside|noscript|iframe|svg|canvas|object|embed|button|select|textarea|dialog|title|", "|"+tag+"|")
			if skipped || container && (hidden && role != "tabpanel" || aria == "true" || strings.Contains("|navigation|banner|contentinfo|complementary|search|", "|"+role+"|")) {
				node.RemoveChild(child)
				child = next
				continue
			}
			clean(child)
			if tag == "head" {
				node.RemoveChild(child)
			} else if (tag == "main" || role == "main") && main == nil && strings.TrimSpace(text(child)) != "" {
				main = child
			} else if tag == "article" {
				articles = append(articles, child)
			}
			child = next
		}
	}
	clean(document)
	convert := func(node *html.Node) (string, error) {
		var serialized strings.Builder
		if err := html.Render(&serialized, node); err != nil {
			return "", err
		}
		engine := converter.NewConverter(converter.WithPlugins(base.NewBasePlugin(), commonmark.NewCommonmarkPlugin(), table.NewTablePlugin()))
		return engine.ConvertString(readableHTML.Sanitize(serialized.String()))
	}
	primary := main
	if primary == nil && len(articles) == 1 {
		primary = articles[0]
	}
	if primary != nil {
		content, err := convert(primary)
		if err != nil {
			return nil, "", webUnavailable("web HTML extraction failed")
		}
		if strings.TrimSpace(content) != "" {
			return title, strings.TrimSpace(content), nil
		}
	}
	content, err := convert(document)
	if err != nil {
		return nil, "", webUnavailable("web HTML extraction failed")
	}
	return title, content, nil
}

func webCharset(value string) string {
	for _, part := range strings.Split(value, ";")[1:] {
		name, value, ok := strings.Cut(part, "=")
		if ok && strings.EqualFold(strings.TrimSpace(name), "charset") {
			return strings.Trim(strings.TrimSpace(value), "\"'")
		}
	}
	return ""
}

func metaCharset(value []byte) string {
	value = value[:min(len(value), 1024)]
	view := make([]rune, len(value))
	for index, value := range value {
		view[index] = rune(value)
	}
	tokens := html.NewTokenizer(strings.NewReader(string(view)))
	for {
		kind := tokens.Next()
		if kind == html.ErrorToken {
			return ""
		}
		if kind != html.StartTagToken && kind != html.SelfClosingTagToken {
			continue
		}
		token := tokens.Token()
		if token.Data != "meta" {
			continue
		}
		attributes := map[string]string{}
		for _, attribute := range token.Attr {
			attributes[attribute.Key] = attribute.Val
		}
		label := attributes["charset"]
		if label == "" && strings.EqualFold(attributes["http-equiv"], "content-type") {
			label = webCharset(attributes["content"])
		}
		if label != "" && len(label) <= 100 && asciiString(label) {
			return label
		}
	}
}
func asciiString(value string) bool {
	for _, value := range []byte(value) {
		if value >= 128 {
			return false
		}
	}
	return true
}

func decodeWebBody(value []byte, contentType string, isHTML bool) string {
	var selected encoding.Encoding
	label := webCharset(contentType)
	if label == "" && isHTML {
		label = metaCharset(value)
	}
	if bytes.HasPrefix(value, []byte{0xef, 0xbb, 0xbf}) {
		value = value[3:]
		selected = unicodeencoding.UTF8
	} else if bytes.HasPrefix(value, []byte{0xff, 0xfe}) {
		value = value[2:]
		selected = unicodeencoding.UTF16(unicodeencoding.LittleEndian, unicodeencoding.IgnoreBOM)
	} else if bytes.HasPrefix(value, []byte{0xfe, 0xff}) {
		value = value[2:]
		selected = unicodeencoding.UTF16(unicodeencoding.BigEndian, unicodeencoding.IgnoreBOM)
	} else if label != "" {
		selected, _ = charset.Lookup(label)
	}
	var decoded string
	if selected != nil {
		result, err := selected.NewDecoder().Bytes(value)
		if err == nil {
			decoded = string(result)
		} else {
			decoded = webUTF8Lossy(value)
		}
	} else if utf8.Valid(value) {
		decoded = string(value)
	} else {
		result, err := simplifiedchinese.GB18030.NewDecoder().Bytes(value)
		if err == nil && strings.ContainsRune(string(result), utf8.RuneError) {
			roundtrip, encodeErr := simplifiedchinese.GB18030.NewEncoder().Bytes(result)
			if encodeErr != nil || !bytes.Equal(roundtrip, value) {
				err = io.ErrUnexpectedEOF
			}
		}
		if err == nil {
			decoded = string(result)
		} else {
			decoded = webUTF8Lossy(value)
		}
	}
	return strings.Map(func(value rune) rune {
		if unicode.IsControl(value) && value != '\t' && value != '\n' && value != '\r' && value != '\f' {
			return utf8.RuneError
		}
		return value
	}, decoded)
}

func webUTF8Lossy(value []byte) string {
	var text strings.Builder
	text.Grow(len(value))
	for len(value) > 0 {
		character, size := utf8.DecodeRune(value)
		if character != utf8.RuneError || size != 1 {
			text.Write(value[:size])
			value = value[size:]
			continue
		}
		// Replace the maximal malformed prefix once, preserving adjacent invalid
		// bytes as separate replacements and incomplete multibyte prefixes as one.
		width := 1
		switch {
		case value[0] >= 0xc2 && value[0] <= 0xdf:
			width = 2
		case value[0] >= 0xe0 && value[0] <= 0xef:
			width = 3
		case value[0] >= 0xf0 && value[0] <= 0xf4:
			width = 4
		}
		prefix := 1
		for prefix < width && prefix < len(value) {
			next := value[prefix]
			if next < 0x80 || next > 0xbf || prefix == 1 && (value[0] == 0xe0 && next < 0xa0 || value[0] == 0xed && next > 0x9f || value[0] == 0xf0 && next < 0x90 || value[0] == 0xf4 && next > 0x8f) {
				break
			}
			prefix++
		}
		text.WriteRune(utf8.RuneError)
		value = value[prefix:]
	}
	return text.String()
}
