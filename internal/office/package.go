// OPC inspection follows Sailry's reviewed bounded package reader.
package office

import (
	"archive/zip"
	"bytes"
	"encoding/xml"
	"github.com/sailry/sailry-harness/internal/protocol"
	"io"
	"path"
	"slices"
	"strings"
	"unicode/utf8"
)

type documentPackage map[string][]byte

func openPackage(data []byte) (documentPackage, error) {
	reader, e := zip.NewReader(bytes.NewReader(data), int64(len(data)))
	if e != nil {
		return nil, invalid(e)
	}
	if len(reader.File) > 4096 {
		return nil, invalid("archive has too many parts")
	}
	p := documentPackage{}
	var total uint64
	for _, file := range reader.File {
		if file.UncompressedSize64 > Limit || total > (128<<20)-file.UncompressedSize64 {
			return nil, invalid("expanded document exceeds the size limit")
		}
		total += file.UncompressedSize64
		if _, exists := p[file.Name]; exists {
			return nil, invalid("duplicate archive part")
		}
		handle, e := file.Open()
		if e != nil {
			return nil, invalid(e)
		}
		content, e := io.ReadAll(io.LimitReader(handle, Limit+1))
		closed := handle.Close()
		if e != nil {
			return nil, invalid(e)
		}
		if closed != nil {
			return nil, invalid(closed)
		}
		if len(content) > Limit {
			return nil, invalid("document part is too large")
		}
		p[file.Name] = content
	}
	return p, nil
}
func (p documentPackage) xml(name string) ([]byte, error) {
	data, ok := p[name]
	if !ok {
		return nil, invalid("missing document part: " + name)
	}
	if !utf8.Valid(data) {
		return nil, invalid("document part is not UTF-8")
	}
	return data, nil
}
func target(base, relative string) (string, error) {
	if strings.ContainsAny(relative, "\\:\x00") {
		return "", invalid("invalid document relationship")
	}
	parts := []string{}
	if !strings.HasPrefix(relative, "/") {
		parts = strings.Split(base, "/")
	}
	for _, part := range strings.Split(relative, "/") {
		switch part {
		case "", ".":
		case "..":
			if len(parts) == 0 {
				return "", invalid("document relationship escapes its package")
			}
			parts = parts[:len(parts)-1]
		default:
			parts = append(parts, part)
		}
	}
	return strings.Join(parts, "/"), nil
}

type relationship struct{ ID, Type, Target string }

func relationships(p documentPackage, name string) ([]relationship, error) {
	data, e := p.xml(name)
	if e != nil {
		return nil, e
	}
	var out []relationship
	e = elements(data, func(start xml.StartElement) error {
		if start.Name.Local == "Relationship" {
			r := relationship{}
			for _, a := range start.Attr {
				switch a.Name.Local {
				case "Id":
					r.ID = a.Value
				case "Type":
					r.Type = a.Value
				case "Target":
					r.Target = a.Value
				}
			}
			out = append(out, r)
		}
		return nil
	})
	return out, e
}
func elements(data []byte, visit func(xml.StartElement) error) error {
	decoder := xml.NewDecoder(bytes.NewReader(data))
	for {
		token, e := decoder.Token()
		if e == io.EOF {
			return nil
		}
		if e != nil {
			return invalid(e)
		}
		if start, ok := token.(xml.StartElement); ok {
			if e := visit(start); e != nil {
				return e
			}
		}
	}
}
func paragraphText(data []byte) (string, error) {
	decoder := xml.NewDecoder(bytes.NewReader(data))
	var paragraphs []strings.Builder
	var active []int
	depth := 0
	textDepth := -1
	for {
		token, e := decoder.Token()
		if e == io.EOF {
			break
		}
		if e != nil {
			return "", invalid(e)
		}
		switch t := token.(type) {
		case xml.StartElement:
			depth++
			if t.Name.Local == "p" {
				paragraphs = append(paragraphs, strings.Builder{})
				active = append(active, len(paragraphs)-1)
			}
			if t.Name.Local == "t" && len(active) > 0 {
				textDepth = depth
			}
		case xml.CharData:
			if textDepth >= 0 && len(active) > 0 {
				paragraphs[active[len(active)-1]].Write([]byte(t))
			}
		case xml.EndElement:
			if depth == textDepth {
				textDepth = -1
			}
			if t.Name.Local == "p" && len(active) > 0 {
				active = active[:len(active)-1]
			}
			depth--
		}
	}
	out := make([]string, len(paragraphs))
	for i := range paragraphs {
		out[i] = paragraphs[i].String()
	}
	return strings.Join(out, "\n"), nil
}
func packageParts(p documentPackage, ext string) ([]Part, error) {
	var ordered []Part
	if ext == "docx" {
		var names []string
		for name := range p {
			if strings.HasSuffix(name, ".xml") && (name == "word/document.xml" || strings.HasPrefix(name, "word/header") || strings.HasPrefix(name, "word/footer")) {
				names = append(names, name)
			}
		}
		slices.Sort(names)
		for _, name := range names {
			ordered = append(ordered, Part{Name: name, Position: protocol.KnowledgePosition{Kind: "section", Data: protocol.KnowledgePositionSectionPayload{Name: name}}})
		}
	} else {
		data, e := p.xml("ppt/presentation.xml")
		if e != nil {
			return nil, e
		}
		rels, e := relationships(p, "ppt/_rels/presentation.xml.rels")
		if e != nil {
			return nil, e
		}
		slide := uint32(0)
		e = elements(data, func(start xml.StartElement) error {
			if start.Name.Local != "sldId" {
				return nil
			}
			slide++
			id := ""
			for _, a := range start.Attr {
				if a.Name.Local == "id" && strings.HasSuffix(a.Name.Space, "/relationships") {
					id = a.Value
				}
			}
			var selected *relationship
			for i := range rels {
				if rels[i].ID == id {
					selected = &rels[i]
					break
				}
			}
			if selected == nil {
				return invalid("slide relationship is missing")
			}
			name, e := target("ppt", selected.Target)
			if e != nil {
				return e
			}
			position := protocol.KnowledgePosition{Kind: "slide", Data: protocol.KnowledgePositionSlidePayload{Slide: slide}}
			ordered = append(ordered, Part{Name: name, Position: position})
			relationName := path.Join(path.Dir(name), "_rels", path.Base(name)+".rels")
			if _, exists := p[relationName]; exists {
				notes, e := relationships(p, relationName)
				if e != nil {
					return e
				}
				for _, note := range notes {
					if strings.HasSuffix(note.Type, "/notesSlide") {
						name, e := target(path.Dir(name), note.Target)
						if e != nil {
							return e
						}
						ordered = append(ordered, Part{Name: name, Position: position})
					}
				}
			}
			return nil
		})
		if e != nil {
			return nil, e
		}
	}
	if len(ordered) == 0 {
		return nil, invalid("document has no readable parts")
	}
	for i := range ordered {
		data, e := p.xml(ordered[i].Name)
		if e != nil {
			return nil, e
		}
		text, e := paragraphText(data)
		if e != nil {
			return nil, e
		}
		ordered[i].Text = text
	}
	return ordered, nil
}
