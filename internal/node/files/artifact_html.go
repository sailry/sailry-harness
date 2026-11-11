package files

import (
	"bytes"
	"encoding/json"
	"image"
	"math"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/MoeForge/moego/base64"
	"github.com/microcosm-cc/bluemonday"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/html"
	"golang.org/x/net/html/atom"
)

func PrepareArtifactHTML(kind protocol.ArtifactKind, source string, allowedClasses []string) (string, []string, error) {
	if strings.TrimSpace(source) == "" || len(source) > artifactHTMLLimit || !utf8.ValidString(source) || !nonnull(source) {
		return "", nil, invalid("artifact HTML is empty or exceeds its limit")
	}
	if kind == protocol.ArtifactKindPreview {
		return source, []string{}, nil
	}
	if kind != protocol.ArtifactKindDocument {
		return "", nil, invalid("invalid artifact kind")
	}
	policy := bluemonday.NewPolicy()
	policy.AllowElements(strings.Fields("section h1 h2 h3 h4 p br hr strong em code pre blockquote ul ol li table thead tbody tr th td caption details summary span div a img figure figcaption form fieldset legend label input textarea select option button")...)
	policy.SkipElementsContent("script", "style", "iframe", "object", "embed", "link", "meta", "base", "svg", "math")
	policy.AllowAttrs("class", "id", "data-title", "data-kind").Globally()
	for tag, names := range map[string]string{"a": "href", "img": "src alt", "th": "colspan rowspan", "td": "colspan rowspan", "details": "open", "form": "data-name", "fieldset": "data-name data-required", "label": "for", "input": "type name value placeholder checked", "textarea": "name placeholder rows", "select": "name multiple", "option": "value selected", "button": "type"} {
		policy.AllowAttrs(strings.Fields(names)...).OnElements(tag)
	}
	policy.RequireParseableURLs(true)
	policy.AllowRelativeURLs(true)
	policy.AllowURLSchemes("data")
	cleaned := policy.Sanitize(source)
	nodes, err := html.ParseFragment(strings.NewReader(cleaned), &html.Node{Type: html.ElementNode, DataAtom: atom.Div, Data: "div"})
	if err != nil {
		return "", nil, invalid("artifact HTML could not be parsed")
	}
	classes := map[string]bool{}
	for _, class := range allowedClasses {
		classes[class] = true
	}
	warnings := []string{}
	invalidChart := false
	var visit func(*html.Node)
	visit = func(node *html.Node) {
		if node.Type == html.ElementNode {
			attributes := node.Attr[:0]
			for _, attribute := range node.Attr {
				keep := len(attribute.Val) <= artifactHTMLLimit
				switch attribute.Key {
				case "href":
					keep = node.Data == "a" && strings.HasPrefix(attribute.Val, "#")
				case "src":
					keep = node.Data == "img" && artifactImage(attribute.Val)
				case "type":
					if node.Data == "input" {
						keep = attribute.Val == "radio" || attribute.Val == "checkbox" || attribute.Val == "text"
					} else if node.Data == "button" {
						keep = attribute.Val == "submit"
					}
				case "class":
					var kept []string
					for _, class := range strings.Fields(attribute.Val) {
						if classes[class] {
							kept = append(kept, class)
						}
					}
					attribute.Val = strings.Join(kept, " ")
					keep = attribute.Val != ""
				case "id", "for":
					keep = artifactIdentifier(attribute.Val, false)
				case "name", "data-name":
					keep = artifactIdentifier(attribute.Val, true)
				case "colspan", "rowspan", "rows":
					number, err := strconv.ParseUint(attribute.Val, 10, 16)
					keep = err == nil && number >= 1 && number <= 100
				}
				if keep {
					attributes = append(attributes, attribute)
				}
			}
			node.Attr = attributes
		}
		for child := node.FirstChild; child != nil; child = child.NextSibling {
			visit(child)
		}
		if node.Type == html.ElementNode && node.Data == "pre" {
			for index, attribute := range node.Attr {
				if attribute.Key != "class" {
					continue
				}
				words := strings.Fields(attribute.Val)
				chart := false
				for _, word := range words {
					chart = chart || word == "chart"
				}
				if !chart {
					continue
				}
				var text strings.Builder
				plain := true
				for child := node.FirstChild; child != nil; child = child.NextSibling {
					if child.Type != html.TextNode {
						plain = false
						break
					}
					text.WriteString(child.Data)
				}
				if !plain || !validArtifactChart(text.String()) {
					kept := words[:0]
					for _, word := range words {
						if word != "chart" {
							kept = append(kept, word)
						}
					}
					node.Attr[index].Val = strings.Join(kept, " ")
					invalidChart = true
				}
			}
		}
	}
	var result bytes.Buffer
	for _, node := range nodes {
		visit(node)
		if err = html.Render(&result, node); err != nil {
			return "", nil, invalid("artifact HTML could not be serialized")
		}
	}
	value := result.String()
	if value != source {
		warnings = append(warnings, "HTML was normalized and unsupported markup was removed")
	}
	if invalidChart {
		warnings = append(warnings, "Invalid chart data was kept as plain text")
	}
	if strings.TrimSpace(value) == "" || len(value) > artifactHTMLLimit {
		return "", nil, invalid("artifact HTML is empty or exceeds its limit after sanitization")
	}
	return value, warnings, nil
}
func artifactIdentifier(value string, field bool) bool {
	maximum := 128
	if field {
		maximum = 64
	}
	if value == "" || len(value) > maximum {
		return false
	}
	for index, b := range []byte(value) {
		letter := b >= 'a' && b <= 'z' || b >= 'A' && b <= 'Z'
		if field && index == 0 && !letter {
			return false
		}
		if letter || b >= '0' && b <= '9' || b == '_' || b == '-' || field && b == '.' {
			continue
		}
		return false
	}
	return true
}
func artifactImage(value string) bool {
	if len(value) > 256<<10 {
		return false
	}
	header, payload, ok := strings.Cut(value, ",")
	if !ok || !strings.HasPrefix(header, "data:image/") || !strings.HasSuffix(header, ";base64") {
		return false
	}
	kind := strings.TrimSuffix(strings.TrimPrefix(header, "data:image/"), ";base64")
	switch kind {
	case "png", "jpeg", "gif", "webp":
	default:
		return false
	}
	data, err := base64.StdEncoding.DecodeString(payload)
	if err != nil || len(data) == 0 {
		return false
	}
	_, format, err := image.DecodeConfig(bytes.NewReader(data))
	return err == nil && format == kind
}
func validArtifactChart(source string) bool {
	if len(source) > 64<<10 {
		return false
	}
	var object map[string]json.RawMessage
	if json.Unmarshal([]byte(source), &object) != nil || object == nil {
		return false
	}
	label := func(value string) bool { return len(value) <= 256 && strings.IndexFunc(value, unicode.IsControl) < 0 }
	number := func(value float64) bool {
		return !math.IsNaN(value) && !math.IsInf(value, 0) && math.Abs(value) <= 1e12
	}
	if title, ok := object["title"]; ok {
		var value string
		if bytes.Equal(bytes.TrimSpace(title), []byte("null")) || json.Unmarshal(title, &value) != nil || !label(value) {
			return false
		}
		delete(object, "title")
	}
	var kind string
	if json.Unmarshal(object["type"], &kind) != nil {
		return false
	}
	switch kind {
	case "line", "bar", "area", "pie", "radar":
		if len(object) != 2 {
			return false
		}
		var points []protocol.ChartPoint
		if json.Unmarshal(object["data"], &points) != nil || len(points) == 0 || len(points) > 500 {
			return false
		}
		positive := false
		for _, point := range points {
			if !label(point.Label) || !number(point.Value) {
				return false
			}
			if kind == "pie" || kind == "radar" {
				if point.Value < 0 {
					return false
				}
				positive = positive || point.Value > 0
			}
		}
		return kind != "pie" && kind != "radar" || positive && (kind != "radar" || len(points) >= 3)
	case "candlestick":
		if len(object) != 2 {
			return false
		}
		var candles []protocol.ChartCandle
		if json.Unmarshal(object["data"], &candles) != nil || len(candles) == 0 || len(candles) > 500 {
			return false
		}
		for _, c := range candles {
			if !label(c.Label) || !number(c.Open) || !number(c.High) || !number(c.Low) || !number(c.Close) || c.Low > min(c.Open, c.Close) || c.High < max(c.Open, c.Close) {
				return false
			}
		}
		return true
	case "sankey":
		if len(object) != 3 {
			return false
		}
		var nodes []protocol.ChartNode
		var links []protocol.ChartLink
		if json.Unmarshal(object["nodes"], &nodes) != nil || json.Unmarshal(object["links"], &links) != nil || len(nodes) == 0 || len(nodes) > 64 || len(links) == 0 || len(links) > 128 {
			return false
		}
		ids := map[string]int{}
		for index, node := range nodes {
			if node.ID == "" || !label(node.ID) || !label(node.Label) {
				return false
			}
			if _, ok := ids[node.ID]; ok {
				return false
			}
			ids[node.ID] = index
		}
		edges := make([][]int, len(nodes))
		degrees := make([]int, len(nodes))
		for _, link := range links {
			from, source := ids[link.Source]
			to, target := ids[link.Target]
			if !source || !target || from == to || !number(link.Value) || link.Value <= 0 {
				return false
			}
			edges[from] = append(edges[from], to)
			degrees[to]++
		}
		var ready []int
		for index, degree := range degrees {
			if degree == 0 {
				ready = append(ready, index)
			}
		}
		visited := 0
		for len(ready) > 0 {
			node := ready[len(ready)-1]
			ready = ready[:len(ready)-1]
			visited++
			for _, next := range edges[node] {
				degrees[next]--
				if degrees[next] == 0 {
					ready = append(ready, next)
				}
			}
		}
		return visited == len(nodes)
	}
	return false
}
