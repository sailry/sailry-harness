package fileview

import (
	"context"
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/localmodels/ocr"
	"github.com/sailry/sailry-harness/internal/office"
)

const (
	pdfPreviewPages = 8
	pdfPreviewEdge  = 1200
)

type Document struct {
	sections []office.Part
	pages    []*ui.Bitmap
	partial  bool
}

func ReadDocument(ctx context.Context, name string, data []byte) (Document, error) {
	var document Document
	if strings.HasSuffix(strings.ToLower(name), ".pdf") {
		pdf, err := ocr.OpenPDF(ctx, data)
		if err != nil {
			return Document{}, err
		}
		defer pdf.Close()
		if pdf.Pages() == 0 {
			return Document{}, fmt.Errorf("PDF has no pages")
		}
		document.partial = pdf.Pages() > pdfPreviewPages
		for index := range min(pdf.Pages(), pdfPreviewPages) {
			if err := ctx.Err(); err != nil {
				return Document{}, err
			}
			page, err := pdf.RenderImage(index, pdfPreviewEdge)
			if err != nil {
				return Document{}, err
			}
			document.pages = append(document.pages, ui.NewBitmap(page))
		}
		return document, nil
	}
	parts, err := office.Parts(name, data)
	if err != nil {
		return Document{}, err
	}
	budget := Limit
	for index, part := range parts {
		if index == 128 || budget == 0 {
			document.partial = true
			break
		}
		if end := min(len(part.Text), office.TextLimit, budget); end < len(part.Text) {
			for end > 0 && !utf8.RuneStart(part.Text[end]) {
				end--
			}
			part.Text = strings.Clone(part.Text[:end])
			document.partial = true
		}
		budget -= len(part.Text)
		document.sections = append(document.sections, part)
	}
	return document, nil
}

func (d Document) Draw(c *ui.Context, key string) {
	p := theme.Of(c)
	ui.Scroll(c.Key(key)).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).Padding(16, 20).Gap(16).Children(func() {
			for index, page := range d.pages {
				width, height := page.Size()
				kit.Image(c.Key(index), page, kit.ImageProps{Fill: true, Contain: true, Variant: kit.ImageBorderless,
					Alt: i18n.T(c, "artifact_preview_page", "page", fmt.Sprint(index+1))}).Role(ui.RoleImage).FillWidth().MaxWidth(float32(width)).AspectRatio(float32(width) / float32(height))
			}
			for index, part := range d.sections {
				ui.Column(c.Key(index)).Gap(6).Children(func() {
					if part.Name != "" {
						ui.Text(c, part.Name).FontWeight(600)
					}
					ui.Text(c, part.Text).Wrap()
				})
			}
			if d.partial {
				ui.Text(c, i18n.T(c, "artifact_preview_partial")).FontSize(12).TextColor(p.MutedForeground)
			}
		})
	})
}
