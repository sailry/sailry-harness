package live

import (
	"bytes"
	"context"
	"encoding/base64"
	"image"
	"image/gif"
	"image/jpeg"
	"image/png"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestDocumentImageFormats(t *testing.T) {
	t.Parallel()
	bitmap := image.NewNRGBA(image.Rect(0, 0, 2, 3))
	for _, format := range []struct {
		name, media string
		encode      func(*bytes.Buffer) error
	}{
		{"png", "image/png", func(out *bytes.Buffer) error { return png.Encode(out, bitmap) }},
		{"jpeg", "image/jpeg", func(out *bytes.Buffer) error { return jpeg.Encode(out, bitmap, nil) }},
		{"gif", "image/gif", func(out *bytes.Buffer) error { return gif.Encode(out, bitmap, nil) }},
		{"svg", "image/svg+xml", func(out *bytes.Buffer) error {
			_, err := out.WriteString(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 3"><path d="M0 0h2v3H0z"/></svg>`)
			return err
		}},
	} {
		t.Run(format.name, func(t *testing.T) {
			var out bytes.Buffer
			if err := format.encode(&out); err != nil {
				t.Fatal(err)
			}
			uri := "data:" + format.media + ";base64," + base64.StdEncoding.EncodeToString(out.Bytes())
			images := &documentImages{ctx: t.Context(), images: make(map[documentImageKey]*documentImage), slots: make(chan struct{}, 2)}
			source, err := images.load(uri)
			if err != nil || source == nil {
				t.Fatalf("load %s: %v", format.name, err)
			}
			if format.name == "svg" {
				if _, ok := source.(*ui.SVG); !ok {
					t.Fatalf("SVG decoded as %T", source)
				}
			} else if _, ok := source.(*ui.Bitmap); !ok {
				t.Fatalf("raster image decoded as %T", source)
			}
			cached, err := images.load(uri)
			if err != nil || cached != source {
				t.Fatalf("image was not reused: %v", err)
			}
			closed, cancel := context.WithCancel(t.Context())
			cancel()
			images.ctx = closed
			if _, err := images.load(uri); err == nil {
				t.Fatal("closed document returned a cached image")
			}
		})
	}
}

func TestDocumentHTMLKeepsImageSandbox(t *testing.T) {
	t.Parallel()
	images := &documentImages{ctx: t.Context(), images: make(map[documentImageKey]*documentImage), slots: make(chan struct{}, 2)}
	result := images.inlineHTML(`<picture><source srcset="https://example.com/a.svg"><img src="../private.svg" srcset="https://example.com/b.svg" alt="Missing image"></picture><script>window.localOnly=true</script>`)
	if strings.Contains(result, "srcset") || strings.Contains(result, "../private.svg") || !strings.Contains(result, `alt="Missing image"`) {
		t.Fatalf("unknown image source escaped the placeholder: %s", result)
	}
	if !strings.Contains(result, "<script>window.localOnly=true</script>") {
		t.Fatal("image rewriting changed the sandboxed document's scripts")
	}
	if !strings.Contains(documentPage(result), "img-src data: blob:") {
		t.Fatal("document image loading relaxed the sandbox policy")
	}
}
