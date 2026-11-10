package embedding

import (
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestVectors checks pooling and normalization on small vectors.
func TestVectors(t *testing.T) {
	// Two tokens of dimension three.
	states := []float32{3, 0, 4, 1, 2, 0}
	t.Run("cls pooling", func(t *testing.T) {
		if got := pool(states, 3, protocol.ModelsPoolingCls); !slices.Equal(got, []float32{3, 0, 4}) {
			t.Fatalf("pool = %v", got)
		}
	})
	t.Run("mean pooling", func(t *testing.T) {
		if got := pool(states, 3, protocol.ModelsPoolingMean); !slices.Equal(got, []float32{2, 1, 2}) {
			t.Fatalf("pool = %v", got)
		}
	})
	t.Run("unit length", func(t *testing.T) {
		vector := []float32{3, 0, 4}
		if err := normalize(vector); err != nil || !slices.Equal(vector, []float32{0.6, 0, 0.8}) {
			t.Fatalf("normalize = %v, %v", vector, err)
		}
	})
	t.Run("zero stays zero", func(t *testing.T) {
		vector := []float32{0, 0}
		if err := normalize(vector); err != nil || !slices.Equal(vector, []float32{0, 0}) {
			t.Fatalf("normalize = %v, %v", vector, err)
		}
	})
	for name, value := range map[string]float32{"NaN": float32(math.NaN()), "infinity": float32(math.Inf(1))} {
		t.Run(name, func(t *testing.T) {
			if err := normalize([]float32{1, value}); err == nil {
				t.Fatal("a nonfinite vector was accepted")
			}
		})
	}
}

// TestLoadWithoutRuntime checks every load failure that precedes inference.
// No onnxruntime library is beside the test binary, so a complete model
// directory ends with the runtime being unavailable.
func TestLoadWithoutRuntime(t *testing.T) {
	transformer := protocol.ModelsFormat{Kind: "transformer", Data: &protocol.ModelsFormatTransformerPayload{Dimension: 2, Pooling: protocol.ModelsPoolingCls}}
	tokens, err := os.ReadFile(filepath.Join("tokenizer", "testdata", "tokenizer.json"))
	if err != nil {
		t.Fatal(err)
	}
	model, err := os.ReadFile(filepath.Join("..", "onnx", "testdata", "matmul.onnx"))
	if err != nil {
		t.Fatal(err)
	}
	// model.graph.initializer.data_location = EXTERNAL
	external := []byte{0x3a, 0x04, 0x2a, 0x02, 0x70, 0x01}
	cases := []struct {
		name    string
		format  protocol.ModelsFormat
		files   map[string][]byte
		failure string
	}{
		{"static format", protocol.ModelsFormat{Kind: "static", Data: &protocol.ModelsFormatStaticPayload{Dimension: 2}}, nil, "not a supported embedding"},
		{"missing tokenizer", transformer, nil, "embedding tokenizer failed"},
		{"missing configuration", transformer, map[string][]byte{"tokenizer.json": tokens}, "embedding file could not be read"},
		{"configuration is not an object", transformer, map[string][]byte{"tokenizer.json": tokens, "config.json": []byte("[]")}, "configuration must be an object"},
		{"external tensor data", transformer, map[string][]byte{"tokenizer.json": tokens, "config.json": []byte("{}"), "model.onnx": external}, "external tensor data"},
		{"absent runtime", transformer, map[string][]byte{"tokenizer.json": tokens, "config.json": []byte("{}"), "model.onnx": model}, "embedding runtime could not load"},
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
