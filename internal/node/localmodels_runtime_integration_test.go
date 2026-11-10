//go:build integration

package node

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ocrNode starts a Node whose profile holds the pinned PP-OCRv6 tiny model
// and whose OCR runs on the pinned runtime. SAILRY_ONNXRUNTIME_DIR holds the
// locked library, SAILRY_OCR_MODEL_DIR the model files and
// SAILRY_LOCALMODELS_FIXTURES the documents and the recorded reference text.
// The library is loaded from that directory instead of the test binary's, so
// the availability check of the packaged location is bypassed.
func ocrNode(t *testing.T) (*Node, string) {
	t.Helper()
	library, model, fixtures := os.Getenv("SAILRY_ONNXRUNTIME_DIR"), os.Getenv("SAILRY_OCR_MODEL_DIR"), os.Getenv("SAILRY_LOCALMODELS_FIXTURES")
	if library == "" || model == "" || fixtures == "" {
		t.Fatal("SAILRY_ONNXRUNTIME_DIR, SAILRY_OCR_MODEL_DIR and SAILRY_LOCALMODELS_FIXTURES must be set")
	}
	if err := onnx.Load(library); err != nil {
		t.Fatal(err)
	}
	definition := localmodels.OCR()[0]
	id := localmodels.ID(&definition)
	profile := filepath.Join(t.TempDir(), "profile")
	installed := filepath.Join(profile, "models", "ocr", id)
	if err := os.MkdirAll(installed, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, asset := range definition.Assets {
		data, err := os.ReadFile(filepath.Join(model, asset.Name))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(installed, asset.Name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if err := assets.WriteMarker(installed, id); err != nil {
		t.Fatal(err)
	}
	capabilities := LocalModelCapabilities()
	recognition := capabilities[protocol.ModelsKindOcr]
	recognition.Available = nil
	capabilities[protocol.ModelsKindOcr] = recognition
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	node, err := StartWithOptions(t.Context(), profile, Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		LocalModels: localmodels.Options{Capabilities: capabilities},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { node.Stop(context.Background()) })
	return node, fixtures
}

// recordedPages returns the reference text of every image or page of a
// recorded output, lines joined by newlines.
func recordedPages(t *testing.T, path string) []string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var recorded []struct {
		Lines []struct {
			Text string `json:"text"`
		} `json:"lines"`
	}
	if err := json.Unmarshal(data, &recorded); err != nil {
		t.Fatal(err)
	}
	pages := make([]string, len(recorded))
	for index, entry := range recorded {
		lines := make([]string, len(entry.Lines))
		for line, value := range entry.Lines {
			lines[line] = value.Text
		}
		pages[index] = strings.Join(lines, "\n")
	}
	return pages
}

// TestImageInspection reads image files through the Node with the installed
// model selected automatically, on the interactive lane of the scheduler.
func TestImageInspection(t *testing.T) {
	node, fixtures := ocrNode(t)
	pages := recordedPages(t, filepath.Join(fixtures, "rust-ocr.json"))
	for index, name := range []string{"english.png", "chinese.png", "rotated.png", "dark.jpg", "small.png"} {
		t.Run(name, func(t *testing.T) {
			data, err := os.ReadFile(filepath.Join(fixtures, "ocr", name))
			if err != nil {
				t.Fatal(err)
			}
			inspection, err := node.inspectDocument(t.Context(), name, data, 0)
			if err != nil {
				t.Fatal(err)
			}
			if len(inspection.Sections) != 1 || inspection.Sections[0].Name != "Image" || inspection.Sections[0].Text != pages[index] || inspection.Next != nil {
				t.Fatalf("inspection = %#v, want text %q", inspection, pages[index])
			}
		})
	}

	t.Run("cancelled", func(t *testing.T) {
		data, err := os.ReadFile(filepath.Join(fixtures, "ocr", "english.png"))
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		_, err = node.inspectDocument(ctx, "english.png", data, 0)
		if fault, ok := err.(*protocol.Fault); !ok || fault.Code != protocol.ErrorCodeCancelled {
			t.Fatalf("inspectDocument error = %v", err)
		}
	})

	t.Run("selected model is not installed", func(t *testing.T) {
		registry := node.localModels.registry
		other := localmodels.OCR()[1]
		id := localmodels.ID(&other)
		settings := registry.Settings()
		settings.Choices[protocol.ModelsKindOcr] = protocol.ModelsLocalChoice{Enabled: true, Model: &id}
		if err := registry.Configure(settings, nil); err != nil {
			t.Fatal(err)
		}
		data, err := os.ReadFile(filepath.Join(fixtures, "ocr", "small.png"))
		if err != nil {
			t.Fatal(err)
		}
		_, err = node.inspectDocument(t.Context(), "small.png", data, 0)
		if fault, ok := err.(*protocol.Fault); !ok || fault.Code != protocol.ErrorCodeNotConfigured {
			t.Fatalf("inspectDocument error = %v", err)
		}
	})
}
