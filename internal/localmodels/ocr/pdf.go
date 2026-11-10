package ocr

import (
	"context"
	"errors"
	"fmt"
	"math"
	"sync"

	"github.com/klippa-app/go-pdfium"
	"github.com/klippa-app/go-pdfium/references"
	"github.com/klippa-app/go-pdfium/requests"
	"github.com/klippa-app/go-pdfium/webassembly"
)

// ErrPageSize reports a PDF page without a finite, positive size.
var ErrPageSize = errors.New("PDF page dimensions are invalid")

// renderer is the process-wide PDFium WebAssembly instance. It is created by
// the first document that needs rasterization and serves one document at a
// time.
var renderer struct {
	sync.Mutex
	pool pdfium.Pool
}

// PDF is an open document whose pages can be rasterized for recognition.
// It holds the renderer until Close.
type PDF struct {
	instance pdfium.Pdfium
	document references.FPDF_DOCUMENT
	pages    int
}

// OpenPDF parses a document for rasterization. The caller must Close it.
func OpenPDF(ctx context.Context, data []byte) (*PDF, error) {
	renderer.Lock()
	opened, err := openPDF(ctx, data)
	if err != nil {
		renderer.Unlock()
		return nil, err
	}
	return opened, nil
}

func openPDF(ctx context.Context, data []byte) (*PDF, error) {
	if renderer.pool == nil {
		// The pool outlives the request that creates it.
		pool, err := webassembly.Init(webassembly.Config{MinIdle: 1, MaxIdle: 1, MaxTotal: 1})
		if err != nil {
			return nil, fmt.Errorf("PDF renderer could not start: %w", err)
		}
		renderer.pool = pool
	}
	instance, err := renderer.pool.GetInstanceWithContext(ctx)
	if err != nil {
		return nil, fmt.Errorf("PDF renderer could not start: %w", err)
	}
	document, err := instance.OpenDocument(&requests.OpenDocument{File: &data})
	if err != nil {
		instance.Close()
		return nil, err
	}
	count, err := instance.FPDF_GetPageCount(&requests.FPDF_GetPageCount{Document: document.Document})
	if err != nil {
		instance.FPDF_CloseDocument(&requests.FPDF_CloseDocument{Document: document.Document})
		instance.Close()
		return nil, err
	}
	return &PDF{instance: instance, document: document.Document, pages: count.PageCount}, nil
}

// Pages is the number of pages.
func (p *PDF) Pages() int { return p.pages }

// Render rasterizes the page at a zero-based index on a white background,
// at twice its point size but with no side above 2000 pixels.
func (p *PDF) Render(index int) (*RGB, error) {
	page := requests.Page{ByIndex: &requests.PageByIndex{Document: p.document, Index: index}}
	size, err := p.instance.GetPageSize(&requests.GetPageSize{Page: page})
	if err != nil {
		return nil, err
	}
	if math.IsNaN(size.Width+size.Height) || math.IsInf(size.Width+size.Height, 0) || size.Width <= 0 || size.Height <= 0 {
		return nil, ErrPageSize
	}
	scale := min(2000/max(size.Width, size.Height), 2)
	width, height := int(size.Width*scale), int(size.Height*scale)
	if width == 0 || height == 0 {
		return nil, ErrPageSize
	}
	rendered, err := p.instance.RenderPageInPixels(&requests.RenderPageInPixels{Page: page, Width: width, Height: height})
	if err != nil {
		return nil, err
	}
	// The pixel buffer belongs to the renderer until Cleanup.
	defer rendered.Cleanup()
	return FromImage(rendered.Result.Image), nil
}

// Close releases the document and the renderer.
func (p *PDF) Close() {
	p.instance.FPDF_CloseDocument(&requests.FPDF_CloseDocument{Document: p.document})
	p.instance.Close()
	renderer.Unlock()
}
