// Package office extracts positioned Office and PDF text without file or OCR access.
package office

import (
	"bytes"
	"fmt"
	pdf "github.com/ledongthuc/pdf"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

const Limit = 32 * 1024 * 1024
const TextLimit = 128 * 1024

type Part struct {
	Name, Text string
	Position   protocol.KnowledgePosition
}

func invalid(message any) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprint("Office: ", message)}
}
func Parts(name string, data []byte) (parts []Part, err error) {
	defer func() {
		if value := recover(); value != nil {
			parts = nil
			err = invalid(fmt.Sprintf("document parser failed: %v", value))
		}
	}()
	if len(data) > Limit {
		return nil, invalid("file exceeds 32 MiB")
	}
	ext := strings.ToLower(strings.TrimPrefix(filepath.Ext(name), "."))
	if bytes.HasPrefix(data, []byte("%PDF-")) {
		ext = "pdf"
	}
	switch ext {
	case "pdf":
		reader, e := pdf.NewReader(bytes.NewReader(data), int64(len(data)))
		if e != nil {
			return nil, invalid(e)
		}
		if !reader.Trailer().Key("Encrypt").IsNull() {
			return nil, invalid("encrypted PDF is not supported")
		}
		for page := 1; page <= reader.NumPage(); page++ {
			text, e := reader.Page(page).GetPlainText(nil)
			if e != nil {
				return nil, invalid(e)
			}
			parts = append(parts, Part{Name: fmt.Sprintf("Page %d", page), Text: text, Position: protocol.KnowledgePosition{Kind: "page", Data: protocol.KnowledgePositionPagePayload{Page: uint32(page)}}})
		}
		return parts, nil
	case "docx", "pptx", "xlsx":
		pkg, e := openPackage(data)
		if e != nil {
			return nil, e
		}
		if ext == "xlsx" {
			return workbookParts(pkg, data)
		}
		return packageParts(pkg, ext)
	default:
		return nil, invalid("supported extensions are .docx, .xlsx, .pptx and .pdf")
	}
}
func Inspect(name string, data []byte, offset uint64) (protocol.OfficeInspection, error) {
	parts, err := Parts(name, data)
	if err != nil {
		out := protocol.DefaultOfficeInspection()
		out.Path = name
		out.Revision = fmt.Sprintf("%x", blake3.Sum256(data))
		return out, err
	}
	return Sections(name, data, parts, offset)
}

// Sections pages extracted parts into an inspection of at most eight
// sections starting at offset. Callers that add recognized text to the parts
// of a document use it directly.
func Sections(name string, data []byte, parts []Part, offset uint64) (protocol.OfficeInspection, error) {
	out := protocol.DefaultOfficeInspection()
	out.Path = name
	sum := blake3.Sum256(data)
	out.Revision = fmt.Sprintf("%x", sum)
	var count uint64
	for _, part := range parts {
		if !utf8.ValidString(part.Text) {
			return out, invalid("extracted text is not UTF-8")
		}
		remaining := part.Text
		for {
			end := min(len(remaining), TextLimit)
			for !utf8.ValidString(remaining[:end]) && end > 0 {
				end--
			}
			if count >= offset && len(out.Sections) < 8 {
				out.Sections = append(out.Sections, protocol.OfficeSection{Name: part.Name, Text: remaining[:end], Truncated: end < len(remaining)})
			}
			count++
			remaining = remaining[end:]
			if remaining == "" {
				break
			}
		}
	}
	if offset > count {
		return out, invalid("section offset is out of range")
	}
	if offset+uint64(len(out.Sections)) < count {
		next := offset + uint64(len(out.Sections))
		out.Next = &next
	}
	return out, nil
}
