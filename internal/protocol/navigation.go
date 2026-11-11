package protocol

import (
	_ "embed"
	"encoding/xml"
	"io"
	"math"
	"slices"
	"strconv"
	"strings"
	"unicode"
)

//go:embed icon_names.txt
var iconNames string

var navigationIcons = strings.Split(strings.TrimSpace(iconNames), "\n")

// kubaiIconNames are the ids of Kubai Icons 0.4.1, which the desktop draws
// through github.com/yldm-tech/mygo-kubai-icons: one id a line, sorted.
//
//go:embed kubai_icon_names.txt
var kubaiIconNames string

var emblemIcons = strings.Split(strings.TrimSpace(kubaiIconNames), "\n")

func (navigation PluginDesktopNavigation) Valid() bool {
	label := func(value string) bool {
		return strings.TrimSpace(value) != "" && len(value) <= 128 && !strings.ContainsFunc(value, unicode.IsControl)
	}
	if !label(navigation.Label) || len(navigation.Locales) > 32 || navigation.Icon != nil && !navigation.Icon.Valid() {
		return false
	}
	for locale, value := range navigation.Locales {
		if len(locale) > 32 || !label(locale) || !label(value) {
			return false
		}
	}
	return true
}

func (icon PluginDesktopIcon) Valid() bool {
	switch icon.Kind {
	case "Name":
		var value string
		switch name := icon.Data.(type) {
		case string:
			value = name
		case *string:
			if name != nil {
				value = *name
			}
		}
		if name, found := strings.CutPrefix(value, "kubai:"); found {
			_, exists := slices.BinarySearch(emblemIcons, name)
			return exists
		}
		name, found := strings.CutPrefix(value, "reicon:")
		if !found {
			return false
		}
		_, exists := slices.BinarySearch(navigationIcons, name)
		return exists
	case "Svg":
		switch value := icon.Data.(type) {
		case PluginDesktopIconSvgPayload:
			return validNavigationSVG(value.Svg)
		case *PluginDesktopIconSvgPayload:
			return value != nil && validNavigationSVG(value.Svg)
		}
	}
	return false
}

func validNavigationSVG(value string) bool {
	if len(value) > 32*1024 {
		return false
	}
	decoder := xml.NewDecoder(strings.NewReader(value))
	root, shapes, nodes, depth := false, 0, 1, 0
	for {
		token, err := decoder.Token()
		if err == io.EOF {
			return root && depth == 0 && shapes > 0
		}
		if err != nil {
			return false
		}
		switch token := token.(type) {
		case xml.Directive:
			return false
		case xml.CharData:
			if depth == 0 && strings.TrimSpace(string(token)) != "" {
				return false
			}
			nodes++
		case xml.Comment, xml.ProcInst:
			nodes++
		case xml.EndElement:
			depth--
		case xml.StartElement:
			nodes++
			if token.Name.Space != "" && token.Name.Space != "http://www.w3.org/2000/svg" {
				return false
			}
			if depth == 0 {
				if root || token.Name.Local != "svg" {
					return false
				}
				root = true
				var viewbox string
				for _, attribute := range token.Attr {
					if attribute.Name.Space == "" && attribute.Name.Local == "viewBox" {
						viewbox = attribute.Value
					}
				}
				coordinates := strings.FieldsFunc(viewbox, func(character rune) bool {
					return character == ',' || character == ' ' || character == '\t' || character == '\r' || character == '\n' || character == '\x0b' || character == '\x0c'
				})
				if len(coordinates) != 4 {
					return false
				}
				for index, coordinate := range coordinates {
					number, err := strconv.ParseFloat(coordinate, 32)
					if err != nil || math.IsNaN(number) || math.IsInf(number, 0) || index >= 2 && number <= 0 {
						return false
					}
				}
			}
			depth++
			switch token.Name.Local {
			case "path", "rect", "circle", "ellipse", "line", "polyline", "polygon":
				shapes++
			case "svg", "g", "defs", "clipPath", "mask", "linearGradient", "radialGradient", "stop", "title", "desc":
			default:
				return false
			}
			attributes := make(map[xml.Name]bool, len(token.Attr))
			for _, attribute := range token.Attr {
				if attributes[attribute.Name] {
					return false
				}
				attributes[attribute.Name] = true
				if attribute.Name.Space == "xmlns" || attribute.Name.Space == "" && attribute.Name.Local == "xmlns" {
					continue
				}
				if attribute.Name.Space != "" || !navigationSVGAttribute(attribute.Name.Local) {
					return false
				}
				content := strings.TrimSpace(attribute.Value)
				if strings.Contains(content, "url(") && !(strings.HasPrefix(content, "url(#") && strings.HasSuffix(content, ")") && strings.Count(content, "url(") == 1) {
					return false
				}
			}
		}
		if nodes > 512 {
			return false
		}
	}
}

func navigationSVGAttribute(name string) bool {
	switch name {
	case "viewBox", "width", "height", "id", "d", "x", "y", "x1", "y1", "x2", "y2", "cx", "cy", "r", "rx", "ry", "fx", "fy", "points", "transform", "fill", "fill-rule", "fill-opacity", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "stroke-miterlimit", "stroke-dasharray", "stroke-dashoffset", "stroke-opacity", "opacity", "clip-path", "clip-rule", "clipPathUnits", "mask", "maskUnits", "maskContentUnits", "gradientUnits", "gradientTransform", "spreadMethod", "offset", "stop-color", "stop-opacity", "preserveAspectRatio", "vector-effect", "color", "version", "role", "aria-hidden", "focusable":
		return true
	}
	return false
}
