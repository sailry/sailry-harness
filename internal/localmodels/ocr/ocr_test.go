package ocr

import (
	"bytes"
	"errors"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// gray returns a one-row image whose pixels have equal channels.
func gray(values ...uint8) *RGB {
	image := NewRGB(len(values), 1)
	for index, value := range values {
		image.Pix[index*3], image.Pix[index*3+1], image.Pix[index*3+2] = value, value, value
	}
	return image
}

// TestProcessing checks the pure steps around the models on small inputs
// whose expected values are derived by hand, then the inputs of an engine.
func TestProcessing(t *testing.T) {
	t.Run("dictionary", func(t *testing.T) {
		tokens, err := parseDictionary([]byte("a\r\nb\n"))
		if err != nil || !slices.Equal(tokens, dictionary{"", "a", "b", " "}) {
			t.Fatalf("parseDictionary = %q, %v", tokens, err)
		}
		for name, data := range map[string]string{
			"empty": "", "blank entry": "a\n\nb\n", "control": "a\tb\n", "long entry": strings.Repeat("a", 65) + "\n", "invalid UTF-8": "a\xff\n",
			"too many entries": strings.Repeat("a\n", 65535),
		} {
			if _, err := parseDictionary([]byte(data)); err == nil {
				t.Errorf("%s dictionary was accepted", name)
			}
		}
	})

	t.Run("CTC decode", func(t *testing.T) {
		tokens := dictionary{"", "a", "b", " "}
		cases := []struct {
			name       string
			rows       [][4]float32
			text       string
			confidence float32
			failure    bool
		}{
			{"repeats collapse and blanks separate", [][4]float32{
				{0.1, 0.9, 0, 0}, {0, 0.7, 0.3, 0}, {1, 0, 0, 0}, {0, 0.6, 0.4, 0}, {0, 0.2, 0.8, 0}, {0.1, 0.1, 0.1, 0.7},
			}, "aab ", 0.75, false},
			{"last of equal maxima", [][4]float32{{0.5, 0.5, 0, 0}}, "a", 0.5, false},
			{"only blanks", [][4]float32{{1, 0, 0, 0}, {0.9, 0.1, 0, 0}}, "", 0, false},
			{"probability above one", [][4]float32{{0, 1.5, 0, 0}}, "", 0, true},
			{"negative probability", [][4]float32{{0, 1, -0.1, 0}}, "", 0, true},
			{"no steps", nil, "", 0, true},
		}
		for _, test := range cases {
			var values []float32
			for _, row := range test.rows {
				values = append(values, row[:]...)
			}
			text, confidence, err := tokens.decode(values, len(test.rows))
			if (err != nil) != test.failure || text != test.text || confidence != test.confidence {
				t.Errorf("%s: decode = %q, %v, %v", test.name, text, confidence, err)
			}
		}
		if _, _, err := tokens.decode(make([]float32, 7), 2); err == nil {
			t.Error("a vocabulary mismatch was accepted")
		}
	})

	t.Run("resize", func(t *testing.T) {
		cases := []struct {
			name   string
			source *RGB
			width  int
			want   []uint8
		}{
			{"same size", gray(1, 2, 3), 3, []uint8{1, 2, 3}},
			{"halving averages", gray(0, 100), 1, []uint8{50}},
			// Weights 0.75, 0.75, 0.25 of 1.75 on each side of the edge.
			{"triangle support", gray(0, 0, 255, 255), 2, []uint8{36, 219}},
			{"constant enlarges", gray(7), 3, []uint8{7, 7, 7}},
		}
		for _, test := range cases {
			resized := Resize(test.source, test.width, 1)
			var got []uint8
			for index := range resized.W {
				got = append(got, resized.Pix[index*3+1])
			}
			if resized.W != test.width || resized.H != 1 || !slices.Equal(got, test.want) {
				t.Errorf("%s: Resize = %v, want %v", test.name, got, test.want)
			}
		}
		if tall := Resize(gray(9), 2, 3); tall.W != 2 || tall.H != 3 || !slices.Equal(tall.Pix, bytes.Repeat([]byte{9}, 18)) {
			t.Errorf("Resize to 2x3 = %v", tall.Pix)
		}
	})

	t.Run("input layout", func(t *testing.T) {
		pixel := &RGB{W: 1, H: 1, Pix: []uint8{255, 0, 0}}
		// Recognition input: BGR planes scaled to [-1, 1], zero padded.
		if got := normalized(pixel, 2, true, true); !slices.Equal(got, []float32{-1, 0, -1, 0, 1, 0}) {
			t.Errorf("normalized = %v", got)
		}
		sizes := []struct {
			width, height int
			fixed         [2]int
			want          [2]int
		}{
			{1920, 1080, [2]int{}, [2]int{544, 960}},
			{10, 10, [2]int{}, [2]int{32, 32}},
			{640, 480, [2]int{}, [2]int{480, 640}},
			{1920, 1080, [2]int{736, 0}, [2]int{736, 960}},
		}
		for _, test := range sizes {
			if height, width := detectionSize(test.width, test.height, test.fixed); [2]int{height, width} != test.want {
				t.Errorf("detectionSize(%d, %d, %v) = %d, %d", test.width, test.height, test.fixed, height, width)
			}
		}
		lines := []struct{ width, height, fixed, content, input int }{
			{100, 20, 0, 240, 320},
			{2000, 10, 0, 3200, 3200},
			{500, 48, 320, 320, 320},
			{1, 100, 0, 1, 320},
		}
		for _, test := range lines {
			if content, input := recognitionSize(test.width, test.height, test.fixed); content != test.content || input != test.input {
				t.Errorf("recognitionSize(%d, %d, %d) = %d, %d", test.width, test.height, test.fixed, content, input)
			}
		}
	})

	t.Run("detection regions", func(t *testing.T) {
		const width, height = 64, 48
		fill := func(values []float32, left, top, right, bottom int, value float32) {
			for y := top; y < bottom; y++ {
				for x := left; x < right; x++ {
					values[y*width+x] = value
				}
			}
		}
		prediction := make([]float32, width*height)
		fill(prediction, 10, 8, 40, 20, 0.9)  // kept
		fill(prediction, 10, 30, 40, 40, 0.4) // above the pixel threshold, below the region score
		fill(prediction, 55, 4, 57, 6, 0.9)   // shorter than the minimum side
		// The region spans pixel centers (10,8)-(39,19): area 319 and
		// perimeter 80 expand it by 319*1.5/80 = 5.98 on every side.
		cases := []struct {
			name          string
			width, height int
			want          []Point
		}{
			{"same size", width, height, []Point{{4, 2}, {45, 2}, {45, 25}, {4, 25}}},
			{"scaled to the source", 2 * width, 2 * height, []Point{{8, 4}, {90, 4}, {90, 50}, {8, 50}}},
			{"scaled down", width / 2, height / 4, []Point{{2, 1}, {22, 1}, {22, 6}, {2, 6}}},
		}
		for _, test := range cases {
			boxes := boxesFromBitmap(prediction, width, height, test.width, test.height)
			if len(boxes) != 1 || !slices.Equal(boxes[0], test.want) {
				t.Errorf("%s: boxes = %v, want %v", test.name, boxes, test.want)
			}
		}
		if boxes := boxesFromBitmap(make([]float32, width*height), width, height, width, height); len(boxes) != 0 {
			t.Errorf("an empty map produced %v", boxes)
		}
	})

	t.Run("reading order", func(t *testing.T) {
		box := func(x, y float32) []Point { return []Point{{x, y}, {x + 5, y}, {x + 5, y + 5}, {x, y + 5}} }
		// The first two share a line: their tops differ by less than ten.
		sorted := sortQuadBoxes([][]Point{box(0, 30), box(50, 0), box(10, 5)})
		var got [][2]float32
		for _, region := range sorted {
			got = append(got, [2]float32{region[0].X, region[0].Y})
		}
		if !slices.Equal(got, [][2]float32{{10, 5}, {50, 0}, {0, 30}}) {
			t.Errorf("order = %v", got)
		}
	})

	t.Run("crop", func(t *testing.T) {
		source := NewRGB(4, 4)
		for index := range 16 {
			source.Pix[index*3] = uint8(index)
		}
		red := func(image *RGB) []uint8 {
			var values []uint8
			for index := range image.W * image.H {
				values = append(values, image.Pix[index*3])
			}
			return values
		}
		square, err := rotateCrop(source, []Point{{1, 1}, {3, 1}, {3, 3}, {1, 3}})
		if err != nil || square.W != 2 || square.H != 2 || !slices.Equal(red(square), []uint8{5, 6, 9, 10}) {
			t.Errorf("axis-aligned crop = %v, %v", square, err)
		}
		// A tall region is turned to read horizontally.
		tall, err := rotateCrop(source, []Point{{0, 0}, {1, 0}, {1, 4}, {0, 4}})
		if err != nil || tall.W != 4 || tall.H != 1 || !slices.Equal(red(tall), []uint8{0, 4, 8, 12}) {
			t.Errorf("tall crop = %v, %v", tall, err)
		}
		if _, err := rotateCrop(source, []Point{{2, 2}, {2, 2}, {2, 2}, {2, 2}}); err == nil {
			t.Error("an empty region was cropped")
		}
		if turned := rotate180(gray(1, 2, 3)); !slices.Equal(red(turned), []uint8{3, 2, 1}) {
			t.Errorf("rotate180 = %v", red(turned))
		}
	})

	t.Run("inputs", inputs)
}

// inputs checks image decoding and every engine failure that precedes
// inference. No onnxruntime library is beside the test binary, so a complete
// model directory ends with the runtime being unavailable.
func inputs(t *testing.T) {
	t.Run("supported names", func(t *testing.T) {
		for name, want := range map[string]bool{
			"scan.png": true, "Photo.JPG": true, "a.b.jpeg": true, "x.webp": true, "x.gif": true,
			"x.bmp": false, "x.pdf": false, "png": false, "x.png.txt": false,
		} {
			if Supported(name) != want {
				t.Errorf("Supported(%q) = %v", name, !want)
			}
		}
	})

	t.Run("decode", func(t *testing.T) {
		picture := image.NewNRGBA(image.Rect(0, 0, 2, 1))
		picture.SetNRGBA(0, 0, color.NRGBA{R: 10, G: 20, B: 30, A: 255})
		picture.SetNRGBA(1, 0, color.NRGBA{R: 200, G: 100, B: 50, A: 128})
		var encoded bytes.Buffer
		if err := png.Encode(&encoded, picture); err != nil {
			t.Fatal(err)
		}
		// Alpha is dropped, not blended.
		decoded, err := Decode(encoded.Bytes())
		if err != nil || decoded.W != 2 || decoded.H != 1 || !slices.Equal(decoded.Pix, []uint8{10, 20, 30, 200, 100, 50}) {
			t.Fatalf("Decode = %v, %v", decoded, err)
		}
		// A GIF header announcing 8000x8000 pixels, with no pixel data.
		huge := []byte("GIF89a\x40\x1f\x40\x1f\x00\x00\x00")
		if _, err := Decode(huge); !errors.Is(err, ErrImageSize) {
			t.Fatalf("Decode error = %v, want %v", err, ErrImageSize)
		}
		if _, err := Decode([]byte("not an image")); err == nil || errors.Is(err, ErrImageSize) {
			t.Fatalf("Decode error = %v", err)
		}
	})

	paddle := protocol.ModelsFormat{Kind: "paddle_ocr", Data: &protocol.ModelsFormatPaddleOcrPayload{Orientation: true}}
	model, err := os.ReadFile(filepath.Join("..", "onnx", "testdata", "matmul.onnx"))
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		name    string
		format  protocol.ModelsFormat
		files   map[string][]byte
		failure string
	}{
		{"other format", protocol.ModelsFormat{Kind: "transformer", Data: &protocol.ModelsFormatTransformerPayload{}}, nil, "unsupported OCR family"},
		{"missing dictionary", paddle, nil, "OCR storage failed"},
		{"oversized dictionary", paddle, map[string][]byte{"dictionary.txt": bytes.Repeat([]byte("a\n"), 256*1024+1)}, "OCR dictionary is too large"},
		{"invalid dictionary", paddle, map[string][]byte{"dictionary.txt": []byte("a\n\n")}, "OCR dictionary is invalid"},
		{"absent runtime", paddle, map[string][]byte{"dictionary.txt": []byte("a\nb\n"), "detector.onnx": model, "recognizer.onnx": model, "orientation.onnx": model}, "ONNX Runtime file is unavailable"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			if test.name == "absent runtime" && onnx.Initialize() == nil {
				t.Skip("an integration test already loaded the runtime")
			}
			directory := t.TempDir()
			for name, content := range test.files {
				if err := os.WriteFile(filepath.Join(directory, name), content, 0o600); err != nil {
					t.Fatal(err)
				}
			}
			engine, err := Load(&protocol.ModelsDefinition{Name: "fixture", Format: test.format}, directory)
			if err == nil || engine != nil || !strings.Contains(err.Error(), test.failure) {
				t.Fatalf("Load error = %v, want %q", err, test.failure)
			}
		})
	}
}
