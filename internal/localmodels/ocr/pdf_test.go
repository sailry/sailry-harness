package ocr

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

// TestRenderPDF rasterizes a two-page PDF of filled rectangles with PDFium.
// It runs longer than a second: compiling the PDFium WebAssembly module is
// the cost of the first document in a process and cannot be made smaller.
func TestRenderPDF(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("testdata", "shapes.pdf"))
	if err != nil {
		t.Fatal(err)
	}
	document, err := OpenPDF(context.Background(), data)
	if err != nil {
		t.Fatal(err)
	}
	if document.Pages() != 2 {
		t.Fatalf("Pages = %d", document.Pages())
	}
	pixel := func(image *RGB, x, y int) [3]uint8 {
		return [3]uint8(image.Pix[(y*image.W+x)*3:])
	}
	white, black, red := [3]uint8{255, 255, 255}, [3]uint8{0, 0, 0}, [3]uint8{255, 0, 0}
	cases := []struct {
		name          string
		page          int
		width, height int
		pixels        map[[2]int][3]uint8
	}{
		// 200x100 points at the 2x ceiling; the rectangle spans the middle.
		{"double scale", 0, 400, 200, map[[2]int][3]uint8{{10, 10}: white, {200, 100}: black, {110, 60}: black, {390, 190}: white}},
		// 1400x1000 points limited to 2000 pixels; the rectangle fills the
		// lower left quarter, so the vertical axis is not flipped.
		{"side limit", 1, 2000, 1428, map[[2]int][3]uint8{{10, 10}: white, {10, 1400}: red, {990, 720}: red, {1010, 700}: white, {1990, 1400}: white}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			image, err := document.Render(test.page)
			if err != nil {
				t.Fatal(err)
			}
			if image.W != test.width || image.H != test.height || len(image.Pix) != image.W*image.H*3 {
				t.Fatalf("size = %dx%d, want %dx%d", image.W, image.H, test.width, test.height)
			}
			for at, want := range test.pixels {
				if got := pixel(image, at[0], at[1]); got != want {
					t.Errorf("pixel %v = %v, want %v", at, got, want)
				}
			}
		})
	}
	document.Close()

	t.Run("reopen and reject", func(t *testing.T) {
		if _, err := OpenPDF(context.Background(), []byte("not a PDF")); err == nil {
			t.Fatal("an invalid document was opened")
		}
		again, err := OpenPDF(context.Background(), data)
		if err != nil {
			t.Fatal(err)
		}
		defer again.Close()
		if _, err := again.Render(2); err == nil {
			t.Fatal("a missing page was rendered")
		}
	})
}
