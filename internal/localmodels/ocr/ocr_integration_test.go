//go:build integration

package ocr

import (
	"encoding/json"
	"errors"
	"math"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// reference is the output recorded from the reference engine for one image.
type reference struct {
	Image string `json:"image"`
	Lines []struct {
		Text       string        `json:"text"`
		Confidence float32       `json:"confidence"`
		Points     [4][2]float32 `json:"points"`
	} `json:"lines"`
}

// integrationEngine loads PP-OCRv6 tiny on the pinned runtime.
// SAILRY_ONNXRUNTIME_DIR holds the locked library, SAILRY_OCR_MODEL_DIR the
// verified model files and SAILRY_LOCALMODELS_FIXTURES the images and the
// recorded reference output.
func integrationEngine(t *testing.T) (*Engine, string) {
	t.Helper()
	library, model, fixtures := os.Getenv("SAILRY_ONNXRUNTIME_DIR"), os.Getenv("SAILRY_OCR_MODEL_DIR"), os.Getenv("SAILRY_LOCALMODELS_FIXTURES")
	if library == "" || model == "" || fixtures == "" {
		t.Fatal("SAILRY_ONNXRUNTIME_DIR, SAILRY_OCR_MODEL_DIR and SAILRY_LOCALMODELS_FIXTURES must be set")
	}
	if err := onnx.Load(library); err != nil {
		t.Fatal(err)
	}
	engine, err := Load(&protocol.ModelsDefinition{
		Name:   "PP-OCRv6 tiny",
		Format: protocol.ModelsFormat{Kind: "paddle_ocr", Data: &protocol.ModelsFormatPaddleOcrPayload{Orientation: true}},
	}, model)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(engine.Close)
	return engine, fixtures
}

func recordings(t *testing.T, path string) []reference {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var recorded []reference
	if err := json.Unmarshal(data, &recorded); err != nil {
		t.Fatal(err)
	}
	return recorded
}

// TestReferenceImages recognizes the fixture images and the page images the
// reference rasterized, and requires the recorded text, regions within one
// pixel and confidences within 0.01.
func TestReferenceImages(t *testing.T) {
	engine, fixtures := integrationEngine(t)
	never := func() bool { return false }
	for _, name := range []string{"rust-ocr.json", "rust-pdf-scan.json", "rust-pdf-vector.json", "rust-pdf-text.json"} {
		for _, recorded := range recordings(t, filepath.Join(fixtures, name)) {
			directory := "ocr"
			if name != "rust-ocr.json" {
				directory = "pdf"
			}
			file := filepath.Base(recorded.Image)
			t.Run(file, func(t *testing.T) {
				data, err := os.ReadFile(filepath.Join(fixtures, directory, file))
				if err != nil {
					t.Fatal(err)
				}
				image, err := Decode(data)
				if err != nil {
					t.Fatal(err)
				}
				lines, err := engine.Recognize(image, never)
				if err != nil {
					t.Fatal(err)
				}
				if len(lines) != len(recorded.Lines) {
					t.Fatalf("%d lines, want %d: %v", len(lines), len(recorded.Lines), lines)
				}
				var region, confidence float64
				for index, line := range lines {
					want := recorded.Lines[index]
					if line.Text != want.Text {
						t.Errorf("line %d = %q, want %q", index, line.Text, want.Text)
					}
					confidence = max(confidence, math.Abs(float64(line.Confidence-want.Confidence)))
					for corner := range 4 {
						for axis := range 2 {
							region = max(region, math.Abs(float64(line.Points[corner][axis]-want.Points[corner][axis])))
						}
					}
				}
				if region > 1 || confidence > 0.01 {
					t.Errorf("regions differ by %.0f px, confidences by %.2g", region, confidence)
				}
				t.Logf("%d lines, regions within %.0f px, confidences within %.2g", len(lines), region, confidence)
			})
		}
	}

	t.Run("cancelled", func(t *testing.T) {
		calls := 0
		_, err := engine.Recognize(NewRGB(64, 64), func() bool { calls++; return calls > 1 })
		if !errors.Is(err, ErrCancelled) {
			t.Fatalf("Recognize error = %v, want %v", err, ErrCancelled)
		}
	})
	t.Run("blank image", func(t *testing.T) {
		lines, err := engine.Recognize(NewRGB(64, 64), never)
		if err != nil || len(lines) != 0 {
			t.Fatalf("Recognize = %v, %v", lines, err)
		}
	})
	t.Run("oversized image", func(t *testing.T) {
		if _, err := engine.Recognize(&RGB{W: MaxImageSide + 1, H: 1, Pix: make([]uint8, (MaxImageSide+1)*3)}, never); err == nil {
			t.Fatal("an oversized image was accepted")
		}
	})
}
