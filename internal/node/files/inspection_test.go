package files

import (
	"bytes"
	"context"
	"fmt"
	"image"
	"image/color"
	"image/gif"
	"image/png"
	"math/rand/v2"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestFileInspectionPages(t *testing.T) {
	t.Parallel()
	root, fs := filesFixture(t)
	ctx := context.Background()
	if err := os.Mkdir(filepath.Join(root, "directory"), 0o700); err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 501; index++ {
		if err := os.WriteFile(filepath.Join(root, fmt.Sprintf("file-%03d.txt", index)), nil, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	page, err := fs.ListDirectory(ctx, "", nil)
	if err != nil || len(page.Entries) != 500 || page.Entries[0].Name != "directory" || page.Next == nil {
		t.Fatalf("first directory page: %+v %v", page, err)
	}
	last, err := fs.ListDirectory(ctx, "", page.Next)
	if err != nil || len(last.Entries) != 2 || last.Next != nil {
		t.Fatalf("last directory page: %+v %v", last, err)
	}
	if err = os.WriteFile(filepath.Join(root, "new.txt"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err = fs.ListDirectory(ctx, "", page.Next); err == nil {
		t.Fatal("stale directory cursor was accepted")
	}
	text := "zero\nneedle\nmiddle\nneedle\nlast\n"
	if err = os.WriteFile(filepath.Join(root, "main.go"), []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	fileType := "go"
	options := protocol.FileSearch{Query: "needle", Mode: protocol.SearchModeContent, Context: 1, Limit: 3, FileType: &fileType}
	result, err := Search(ctx, fs, options)
	if err != nil || len(result.Matches) != 3 || result.NextOffset == nil || *result.NextOffset != 3 || result.Matches[0].Matched || !result.Matches[1].Matched {
		t.Fatalf("search page: %+v %v", result, err)
	}
	options.Offset = 3
	result, err = Search(ctx, fs, options)
	if err != nil || len(result.Matches) != 2 || result.NextOffset != nil || result.Matches[0].LineNumber != 4 {
		t.Fatalf("search continuation: %+v %v", result, err)
	}
	options.Offset = 0
	options.Mode = protocol.SearchModeCount
	result, err = Search(ctx, fs, options)
	if err != nil || len(result.Counts) != 1 || result.Counts[0].Count != 2 {
		t.Fatalf("search count: %+v %v", result, err)
	}
}
func TestImagePreparation(t *testing.T) {
	t.Parallel()
	source := image.NewNRGBA(image.Rect(0, 0, 3, 2))
	source.Set(0, 0, color.NRGBA{R: 255, A: 255})
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, source); err != nil {
		t.Fatal(err)
	}
	facts, sent, data, err := PrepareImage(encoded.Bytes())
	if err != nil || facts.Width != 3 || sent != nil || !bytes.Equal(data, encoded.Bytes()) {
		t.Fatalf("small image: %+v %+v %v", facts, sent, err)
	}
	palette := color.Palette{color.Black, color.White}
	frame := image.NewPaletted(image.Rect(0, 0, 3, 2), palette)
	encoded.Reset()
	if err = gif.EncodeAll(&encoded, &gif.GIF{Image: []*image.Paletted{frame, frame}, Delay: []int{1, 1}}); err != nil {
		t.Fatal(err)
	}
	_, sent, data, err = PrepareImage(encoded.Bytes())
	if err != nil || sent == nil || sent.MimeType != "image/png" || bytes.HasPrefix(data, []byte("GIF")) {
		t.Fatalf("animated image: %+v %v", sent, err)
	}
	rotated := orientImage(source, 6)
	if rotated.Bounds().Dx() != 2 || rotated.Bounds().Dy() != 3 || rotated.At(1, 0) != source.At(0, 0) {
		t.Fatalf("orientation was not applied: %v", rotated.Bounds())
	}
	if _, _, _, err = PrepareImage([]byte("not an image")); err == nil {
		t.Fatal("invalid image was accepted")
	}
	t.Run("a capture above the inspection limit is reduced", func(t *testing.T) {
		// Noise does not compress, so this capture is larger than a file
		// image may be and still too large after its side is bounded.
		noise := image.NewNRGBA(image.Rect(0, 0, 2000, 1500))
		random := rand.New(rand.NewPCG(1, 2))
		for index := range noise.Pix {
			noise.Pix[index] = byte(random.Uint32())
			if index%4 == 3 {
				noise.Pix[index] = 255
			}
		}
		var capture bytes.Buffer
		if err := png.Encode(&capture, noise); err != nil {
			t.Fatal(err)
		}
		if capture.Len() <= protocol.MAX_IMAGE_BYTES || capture.Len() > MaxCaptureBytes {
			t.Fatalf("fixture is %d bytes", capture.Len())
		}
		if _, _, _, err := PrepareImage(capture.Bytes()); err == nil {
			t.Fatal("a file image above the inspection limit was accepted")
		}
		facts, sent, data, err := PrepareCapture(capture.Bytes())
		if err != nil || sent == nil {
			t.Fatalf("capture was refused: %+v %v", sent, err)
		}
		config, _, err := image.DecodeConfig(bytes.NewReader(data))
		if err != nil || uint32(config.Width) != sent.Width || uint32(config.Height) != sent.Height {
			t.Fatalf("reduced capture does not decode as reported: %+v %+v %v", config, sent, err)
		}
		if facts.Width != 2000 || facts.Height != 1500 || sent.Width > protocol.MAX_IMAGE_SIDE || sent.Width < minimumImageSide ||
			sent.Width*3 != sent.Height*4 && sent.Width*3/4 != sent.Height || len(data) > sentImageBytes || sent.Size != uint64(len(data)) {
			t.Fatalf("reduced capture: source %+v sent %+v, %d bytes", facts, sent, len(data))
		}
	})
	t.Run("a capture above 20 MiB is refused", func(t *testing.T) {
		if _, _, _, err := PrepareCapture(make([]byte, MaxCaptureBytes+1)); err == nil {
			t.Fatal("an oversized capture was accepted")
		}
	})
}
