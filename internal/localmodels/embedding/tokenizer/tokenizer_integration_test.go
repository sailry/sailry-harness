//go:build integration

package tokenizer

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"
)

// TestReferenceTokens compares IDs, offsets and special IDs with the output
// recorded from the reference tokenizer for the granite-multi tokenizer.json.
// SAILRY_EMBEDDING_MODEL_DIR holds the model files and
// SAILRY_LOCALMODELS_FIXTURES the recorded outputs.
func TestReferenceTokens(t *testing.T) {
	model, fixtures := os.Getenv("SAILRY_EMBEDDING_MODEL_DIR"), os.Getenv("SAILRY_LOCALMODELS_FIXTURES")
	if model == "" || fixtures == "" {
		t.Fatal("SAILRY_EMBEDDING_MODEL_DIR and SAILRY_LOCALMODELS_FIXTURES must be set")
	}
	tokens, err := Load(filepath.Join(model, "tokenizer.json"))
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"rust-tokens.json", "rust-tokens-control.json", "rust-corpus-tokens.json"} {
		t.Run(name, func(t *testing.T) {
			var cases []struct {
				Text       string   `json:"text"`
				IDs        []int    `json:"ids"`
				Offsets    [][2]int `json:"offsets"`
				SpecialIDs []int    `json:"special_ids"`
			}
			data, err := os.ReadFile(filepath.Join(fixtures, name))
			if err != nil {
				t.Fatal(err)
			}
			if err := json.Unmarshal(data, &cases); err != nil {
				t.Fatal(err)
			}
			total := 0
			for _, reference := range cases {
				plain, err := tokens.Encode(reference.Text, false)
				if err != nil {
					t.Fatal(err)
				}
				special, err := tokens.Encode(reference.Text, true)
				if err != nil {
					t.Fatal(err)
				}
				total += len(reference.IDs)
				if !slices.Equal(plain.IDs, reference.IDs) || !slices.Equal(plain.Offsets, reference.Offsets) || !slices.Equal(special.IDs, reference.SpecialIDs) {
					t.Errorf("%.80q: got %v %v, want %v %v", reference.Text, plain.IDs, plain.Offsets, reference.IDs, reference.Offsets)
				}
			}
			t.Logf("%d texts, %d tokens", len(cases), total)
		})
	}
}
