package tokenizer

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

// fixture writes testdata/tokenizer.json, optionally changed, and returns its path.
func fixture(t *testing.T, change func(document map[string]any)) string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "tokenizer.json"))
	if err != nil {
		t.Fatal(err)
	}
	var document map[string]any
	if err := json.Unmarshal(data, &document); err != nil {
		t.Fatal(err)
	}
	if change != nil {
		change(document)
	}
	if data, err = json.Marshal(document); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "tokenizer.json")
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

// TestEncode checks IDs and byte offsets on a small synthetic vocabulary.
// The expectations are derived by hand from the tokenizer.json semantics;
// the dropped-byte cases follow offsets recorded from the reference
// tokenizer for the same situation.
func TestEncode(t *testing.T) {
	type spans = [][2]int
	cases := []struct {
		name, text string
		ids        []int
		offsets    spans
	}{
		{"whole word in the vocabulary", "hello", []int{26}, spans{{0, 5}}},
		{"leading space joins the word", "hello world", []int{26, 7, 4, 3, 5, 2, 6}, spans{{0, 5}, {5, 6}, {6, 7}, {7, 8}, {8, 9}, {9, 10}, {10, 11}}},
		{"newline is its own unknown word", "hello\nworld", []int{26, 29}, spans{{0, 5}, {6, 11}}},
		{"merges in rank order", "helo hellow", []int{23, 2, 3, 7, 26, 4}, spans{{0, 2}, {2, 3}, {3, 4}, {4, 5}, {5, 10}, {10, 11}}},
		{"lower rank before leftmost", "llhe", []int{24, 23}, spans{{0, 2}, {2, 4}}},
		{"multi-byte word and punctuation", "é!", []int{27, 8}, spans{{0, 2}, {2, 3}}},
		{"token inside a character reports the character", "ü", []int{13, 15}, spans{{0, 2}, {0, 2}}},
		{"digit groups", "1234", []int{28, 11, 12}, spans{{0, 2}, {2, 3}, {3, 4}}},
		{"added token splits text", "hello<sep>world", []int{26, 34, 29}, spans{{0, 5}, {5, 10}, {10, 15}}},
		{
			"single-word added token needs boundaries", "a<tag>b <tag> x",
			[]int{16, 20, 18, 16, 19, 21, 17, 7, 33, 7, 22},
			spans{{0, 1}, {1, 2}, {2, 3}, {3, 4}, {4, 5}, {5, 6}, {6, 7}, {7, 8}, {8, 13}, {13, 14}, {14, 15}},
		},
		{"dropped byte takes no position", " \f ", []int{30}, spans{{0, 2}}},
		{"dropped byte shifts the word", "\fb", []int{17}, spans{{0, 1}}},
		{"empty", "", nil, nil},
	}
	for _, path := range []struct {
		name   string
		change func(map[string]any)
	}{
		{"pair merges", nil},
		{"legacy merges", func(document map[string]any) {
			document["model"].(map[string]any)["merges"] = []string{"h e", "l l", "he ll", "hell o", "Ã ©", "1 2", "Ġ Ġ"}
		}},
	} {
		tokens, err := Load(fixture(t, path.change))
		if err != nil {
			t.Fatalf("%s: %v", path.name, err)
		}
		for _, test := range cases {
			t.Run(path.name+"/"+test.name, func(t *testing.T) {
				plain, err := tokens.Encode(test.text, false)
				if err != nil {
					t.Fatal(err)
				}
				if !reflect.DeepEqual(plain.IDs, test.ids) || !reflect.DeepEqual(plain.Offsets, test.offsets) {
					t.Fatalf("Encode(%q) = %v %v, want %v %v", test.text, plain.IDs, plain.Offsets, test.ids, test.offsets)
				}
				special, err := tokens.Encode(test.text, true)
				if err != nil {
					t.Fatal(err)
				}
				ids := append(append([]int{31}, test.ids...), 32)
				offsets := append(append(spans{{}}, test.offsets...), [2]int{})
				if !reflect.DeepEqual(special.IDs, ids) || !reflect.DeepEqual(special.Offsets, offsets) {
					t.Fatalf("Encode(%q, special) = %v %v, want %v %v", test.text, special.IDs, special.Offsets, ids, offsets)
				}
			})
		}
		t.Run(path.name+"/invalid UTF-8", func(t *testing.T) {
			if _, err := tokens.Encode("a\xff", false); err == nil {
				t.Fatal("invalid UTF-8 was encoded")
			}
		})
	}
}

// TestLoadRejects checks that components outside the supported subset fail
// to load instead of being approximated.
func TestLoadRejects(t *testing.T) {
	at := func(document map[string]any, keys ...string) map[string]any {
		for _, key := range keys {
			document = document[key].(map[string]any)
		}
		return document
	}
	cases := []struct {
		name    string
		change  func(map[string]any)
		failure string
	}{
		{"normalizer", func(d map[string]any) { d["normalizer"] = map[string]any{"type": "BertNormalizer"} }, "normalizer"},
		{"model type", func(d map[string]any) { at(d, "model")["type"] = "WordPiece" }, "model"},
		{"unknown token", func(d map[string]any) { at(d, "model")["unk_token"] = "[UNK]" }, "model"},
		{"pre-tokenizer", func(d map[string]any) { d["pre_tokenizer"] = map[string]any{"type": "ByteLevel"} }, "pre-tokenizer"},
		{"split pattern", func(d map[string]any) {
			at(d, "pre_tokenizer")["pretokenizers"].([]any)[0].(map[string]any)["pattern"] = map[string]any{"Regex": "("}
		}, "split pattern is invalid"},
		{"added token option", func(d map[string]any) { d["added_tokens"].([]any)[0].(map[string]any)["lstrip"] = true }, "added token option"},
		{"post-processor", func(d map[string]any) { at(d, "post_processor")["type"] = "BertProcessing" }, "post-processor"},
		{"template", func(d map[string]any) { at(d, "post_processor")["single"] = []any{} }, "post-processor template"},
		{"merge", func(d map[string]any) { at(d, "model")["merges"] = [][2]string{{"h", "q"}} }, "merge is missing"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			if _, err := Load(fixture(t, test.change)); err == nil || !strings.Contains(err.Error(), test.failure) {
				t.Fatalf("Load error = %v, want %q", err, test.failure)
			}
		})
	}
}
