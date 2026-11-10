//go:build integration

package embedding

import (
	"encoding/json"
	"errors"
	"math"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func read(t *testing.T, path string, target any) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, target); err != nil {
		t.Fatal(err)
	}
}

// compare returns the largest component difference and the cosine similarity.
func compare(actual, expected []float32) (float64, float64) {
	var difference, dot, left, right float64
	for index := range actual {
		difference = max(difference, math.Abs(float64(actual[index]-expected[index])))
		dot += float64(actual[index]) * float64(expected[index])
		left += float64(actual[index]) * float64(actual[index])
		right += float64(expected[index]) * float64(expected[index])
	}
	return difference, dot / math.Sqrt(left*right)
}

// TestGraniteReference runs granite-multi on the pinned runtime and compares
// vectors with the committed reference (tolerance 5e-4, cosine >= 0.99) and
// with vectors and offsets recorded from the reference engine.
// SAILRY_ONNXRUNTIME_DIR holds the locked library, SAILRY_EMBEDDING_MODEL_DIR
// the verified model files and SAILRY_LOCALMODELS_FIXTURES the recorded output.
func TestGraniteReference(t *testing.T) {
	library, model, fixtures := os.Getenv("SAILRY_ONNXRUNTIME_DIR"), os.Getenv("SAILRY_EMBEDDING_MODEL_DIR"), os.Getenv("SAILRY_LOCALMODELS_FIXTURES")
	if library == "" || model == "" || fixtures == "" {
		t.Fatal("SAILRY_ONNXRUNTIME_DIR, SAILRY_EMBEDDING_MODEL_DIR and SAILRY_LOCALMODELS_FIXTURES must be set")
	}
	if err := onnx.Load(library); err != nil {
		t.Fatal(err)
	}
	definition := protocol.ModelsDefinition{
		Name:   "Granite Multilingual",
		Format: protocol.ModelsFormat{Kind: "transformer", Data: &protocol.ModelsFormatTransformerPayload{Dimension: 384, Pooling: protocol.ModelsPoolingCls}},
	}
	engine, err := Load(&definition, model)
	if err != nil {
		t.Fatal(err)
	}
	defer engine.Close()
	if engine.Dimension() != 384 {
		t.Fatalf("dimension = %d", engine.Dimension())
	}

	t.Run("committed reference", func(t *testing.T) {
		var reference struct {
			Cases []struct {
				Text   string    `json:"text"`
				Vector []float32 `json:"vector"`
			} `json:"cases"`
		}
		read(t, filepath.Join("testdata", "granite-multi.json"), &reference)
		texts := make([]string, len(reference.Cases))
		for index, entry := range reference.Cases {
			texts[index] = entry.Text
		}
		vectors, err := engine.Encode(Document, texts)
		if err != nil || len(vectors) != len(texts) || len(texts) < 3 {
			t.Fatalf("Encode = %d vectors, %v", len(vectors), err)
		}
		worst := 0.0
		for index, entry := range reference.Cases {
			difference, cosine := compare(vectors[index], entry.Vector)
			worst = max(worst, difference)
			if len(vectors[index]) != len(entry.Vector) || difference >= 5e-4 || cosine < 0.99 {
				t.Errorf("%q differs by %g, cosine %f", entry.Text, difference, cosine)
			}
		}
		t.Logf("%d texts, largest difference %.3g", len(texts), worst)
		if empty, err := engine.Encode(Document, nil); err != nil || len(empty) != 0 {
			t.Fatalf("Encode(nil) = %v, %v", empty, err)
		}
	})

	t.Run("recorded engine output", func(t *testing.T) {
		var recorded struct {
			Texts   []string    `json:"texts"`
			Vectors [][]float32 `json:"vectors"`
			Offsets [][][2]int  `json:"offsets"`
		}
		read(t, filepath.Join(fixtures, "rust-embed.json"), &recorded)
		vectors, err := engine.Encode(Document, recorded.Texts)
		if err != nil {
			t.Fatal(err)
		}
		worst := 0.0
		for index, text := range recorded.Texts {
			difference, _ := compare(vectors[index], recorded.Vectors[index])
			worst = max(worst, difference)
			if difference >= 5e-4 {
				t.Errorf("%q differs by %g", text, difference)
			}
			offsets, err := engine.Offsets(text)
			if err != nil || !slices.Equal(offsets, recorded.Offsets[index]) {
				t.Errorf("Offsets(%q) = %v, %v, want %v", text, offsets, err, recorded.Offsets[index])
			}
		}
		t.Logf("%d texts, largest difference %.3g", len(recorded.Texts), worst)
	})

	t.Run("window", func(t *testing.T) {
		if _, err := engine.Encode(Document, []string{strings.Repeat("alpha ", MaxInputTokens)}); !errors.Is(err, ErrTooLong) {
			t.Fatalf("Encode error = %v, want %v", err, ErrTooLong)
		}
	})

	t.Run("prefixes", func(t *testing.T) {
		query, document := "query: ", "passage: "
		prefixed := definition
		prefixed.QueryPrefix, prefixed.DocumentPrefix = &query, &document
		other, err := Load(&prefixed, model)
		if err != nil {
			t.Fatal(err)
		}
		defer other.Close()
		for input, prefix := range map[Input]string{Query: query, Document: document} {
			got, err := other.Encode(input, []string{"alpha beta"})
			if err != nil {
				t.Fatal(err)
			}
			want, err := engine.Encode(Document, []string{prefix + "alpha beta"})
			if err != nil {
				t.Fatal(err)
			}
			if !slices.Equal(got[0], want[0]) {
				t.Errorf("prefix %q is not applied", prefix)
			}
		}
		long := strings.Repeat("alpha ", MaxInputTokens/2)
		prefixed.DocumentPrefix = &long
		if _, err := Load(&prefixed, model); err == nil || !strings.Contains(err.Error(), "document prefix") {
			t.Fatalf("Load error = %v", err)
		}
	})
}
