package node

import (
	"bytes"
	"context"
	"errors"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/localmodels/ocr"
	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// inspectDocument reads a document for the agent. Image files and PDF pages
// without a text layer are read with the OCR model the settings select.
func (node *Node) inspectDocument(ctx context.Context, name string, data []byte, offset uint64) (protocol.OfficeInspection, error) {
	parts, err := node.documentParts(ctx, name, data)
	if err != nil {
		return protocol.DefaultOfficeInspection(), err
	}
	return office.Sections(name, data, parts, offset)
}

func (node *Node) documentParts(ctx context.Context, name string, data []byte) ([]office.Part, error) {
	if ocr.Supported(name) {
		return node.imageParts(ctx, data)
	}
	parts, err := office.Parts(name, data)
	if err != nil {
		return nil, err
	}
	if strings.EqualFold(filepath.Ext(name), ".pdf") || bytes.HasPrefix(data, []byte("%PDF-")) {
		return node.scannedParts(ctx, data, parts)
	}
	return parts, nil
}

// scannedParts fills the pages of a PDF that have no text with recognized
// text. Without a configured OCR model the pages stay as extracted.
func (node *Node) scannedParts(ctx context.Context, data []byte, parts []office.Part) ([]office.Part, error) {
	blank := func(part office.Part) bool { return strings.TrimSpace(part.Text) == "" }
	scanned := false
	for _, part := range parts {
		scanned = scanned || blank(part)
	}
	if !scanned {
		return parts, nil
	}
	resource, err := node.ocrResource()
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
	if document.Pages() != len(parts) {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "PDF page count changed during extraction"}
	}
	for index := range parts {
		if !blank(parts[index]) {
			continue
		}
		if ctx.Err() != nil {
			return nil, ocrCancelled()
		}
		image, err := document.Render(index)
		if errors.Is(err, ocr.ErrPageSize) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: err.Error()}
		}
		if err != nil {
			return nil, ocrFailure(err)
		}
		text, err := node.recognize(ctx, resource, image)
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
