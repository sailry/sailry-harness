package node

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"

	"github.com/MoeForge/moego/base64"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/ocr"
	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const maxPDFRenderPages = 4

// inspectDocument reads a document for the agent. Image files and PDF pages
// without a text layer are read with the OCR model the settings select.
func (node *Node) inspectDocument(ctx context.Context, name string, data []byte, offset uint64) (protocol.OfficeInspection, error) {
	return node.inspectDocumentOptions(ctx, name, data, protocol.OfficeRead{Offset: offset})
}

// inspectDocumentOptions reads document text by default, or handles PDF-specific
// text/image modes requested by read_pdf.
func (node *Node) inspectDocumentOptions(ctx context.Context, name string, data []byte, options protocol.OfficeRead) (protocol.OfficeInspection, error) {
	mode := strings.ToLower(strings.TrimSpace(options.Mode))
	switch mode {
	case "", "text", "images":
	default:
		return protocol.DefaultOfficeInspection(), &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF: mode must be \"text\" or \"images\""}
	}
	if (mode != "" || len(options.Pages) != 0) && !office.IsPDF(name, data) {
		return protocol.DefaultOfficeInspection(), &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF: expected a PDF document; use read_office for Office files"}
	}
	if mode == "images" {
		return node.renderPDFPages(ctx, name, data, options.Offset, options.Pages)
	}
	if len(options.Pages) > 0 {
		parts, total, err := office.PDFPages(name, data, options.Pages)
		if err != nil {
			return protocol.DefaultOfficeInspection(), err
		}
		parts, err = node.scannedParts(ctx, data, parts, localmodels.Interactive)
		if err != nil {
			return protocol.DefaultOfficeInspection(), err
		}
		out, err := office.Sections(name, data, parts, options.Offset)
		if err != nil {
			return out, err
		}
		out.TotalPages = &total
		return out, nil
	}
	parts, err := node.documentParts(ctx, name, data, localmodels.Interactive)
	if err != nil {
		return protocol.DefaultOfficeInspection(), err
	}
	return office.Sections(name, data, parts, options.Offset)
}

func (node *Node) renderPDFPages(ctx context.Context, name string, data []byte, offset uint64, pages []uint32) (protocol.OfficeInspection, error) {
	out := protocol.DefaultOfficeInspection()
	out.Path = name
	if len(data) > office.Limit {
		return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF: file exceeds 32 MiB"}
	}
	if !office.IsPDF(name, data) {
		return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF: page image rendering is only supported for PDF documents"}
	}
	document, err := ocr.OpenPDF(ctx, data)
	if err != nil {
		return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("PDF: %v", err)}
	}
	defer document.Close()
	total := uint32(document.Pages())
	if total == 0 {
		return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF: document has no pages"}
	}
	var unique []uint32
	var next *uint64
	if len(pages) == 0 {
		if offset >= uint64(total) {
			return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("PDF: page offset %d is out of range (total pages: %d)", offset, total)}
		}
		start := uint32(offset) + 1
		end := min(total, start+maxPDFRenderPages-1)
		unique = make([]uint32, 0, end-start+1)
		for p := start; p <= end; p++ {
			unique = append(unique, p)
		}
		if end < total {
			n := uint64(end)
			next = &n
		}
	} else {
		unique = make([]uint32, 0, len(pages))
		for _, page := range pages {
			if page == 0 {
				return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF: page numbers are 1-based"}
			}
			if !slices.Contains(unique, page) {
				unique = append(unique, page)
			}
		}
		if len(unique) > maxPDFRenderPages {
			return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("PDF: at most %d pages can be rendered per request", maxPDFRenderPages)}
		}
		for _, page := range unique {
			if page > total {
				return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("PDF: page %d is out of range (1..%d)", page, total)}
			}
		}
	}
	sum := blake3.Sum256(data)
	out.Revision = fmt.Sprintf("%x", sum)
	out.TotalPages = &total
	out.Next = next
	out.Sections = make([]protocol.OfficeSection, 0, len(unique))
	out.InlineData = make([]protocol.InlineImage, 0, len(unique))
	for _, page := range unique {
		if ctx.Err() != nil {
			return out, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "PDF page rendering was cancelled"}
		}
		rendered, err := document.RenderImage(int(page-1), protocol.MAX_IMAGE_SIDE)
		if errors.Is(err, ocr.ErrPageSize) {
			return out, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: fmt.Sprintf("PDF: %v", err)}
		}
		if err != nil {
			return out, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: fmt.Sprintf("PDF: failed to render page %d: %v", page, err)}
		}
		mime, encoded, width, height, err := files.EncodePreparedImage(rendered)
		if err != nil {
			return out, err
		}
		out.InlineData = append(out.InlineData, protocol.InlineImage{
			MimeType: mime,
			Data:     base64.StdEncoding.EncodeToString(encoded),
		})
		out.Sections = append(out.Sections, protocol.OfficeSection{
			Name: fmt.Sprintf("Page %d (image)", page),
			Text: fmt.Sprintf("Rendered PDF page %d of %d as %s (%dx%d).", page, total, mime, width, height),
		})
	}
	return out, nil
}

// documentParts extracts the parts of a document; lane orders its OCR work.
func (node *Node) documentParts(ctx context.Context, name string, data []byte, lane localmodels.Lane) ([]office.Part, error) {
	if ocr.Supported(name) {
		return node.imageParts(ctx, data, lane)
	}
	parts, err := office.Parts(name, data)
	if err != nil {
		return nil, err
	}
	if office.IsPDF(name, data) {
		return node.scannedParts(ctx, data, parts, lane)
	}
	return parts, nil
}

// scannedParts fills the pages of a PDF that have no text with recognized
// text. Without a configured OCR model the pages stay as extracted.
func (node *Node) scannedParts(ctx context.Context, data []byte, parts []office.Part, lane localmodels.Lane) ([]office.Part, error) {
	blank := func(part office.Part) bool { return strings.TrimSpace(part.Text) == "" }
	scanned := false
	for _, part := range parts {
		scanned = scanned || blank(part)
	}
	if !scanned {
		return parts, nil
	}
	resource, err := node.ocrResource(ctx)
	if notConfigured(err) {
		return parts, nil
	}
	if err != nil {
		return nil, err
	}
	document, err := ocr.OpenPDF(ctx, data)
	if err != nil {
		return nil, ocrFailure(err)
	}
	defer document.Close()
	total := document.Pages()
	for index := range parts {
		if !blank(parts[index]) {
			continue
		}
		pageIdx := index
		if parts[index].Position.Kind == "page" {
			if payload, ok := parts[index].Position.Data.(protocol.KnowledgePositionPagePayload); ok && payload.Page > 0 {
				pageIdx = int(payload.Page - 1)
			}
		}
		if pageIdx < 0 || pageIdx >= total {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF page count changed during extraction"}
		}
		if ctx.Err() != nil {
			return nil, ocrCancelled()
		}
		image, err := document.Render(pageIdx)
		if errors.Is(err, ocr.ErrPageSize) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: err.Error()}
		}
		if err != nil {
			return nil, ocrFailure(err)
		}
		text, err := node.recognize(ctx, resource, image, lane)
		if notConfigured(err) {
			return parts, nil
		}
		if err != nil {
			return nil, err
		}
		parts[index].Text = text
	}
	return parts, nil
}
