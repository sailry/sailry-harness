// Package tokenizer reads the Hugging Face tokenizer.json components the
// pinned granite-multi embedding model uses and reproduces tokenizers 0.23.2
// for them: added tokens, Split (Isolated) followed by ByteLevel
// pre-tokenization, BPE with ignore_merges and TemplateProcessing. Any other
// component is rejected instead of being approximated.
package tokenizer

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/dlclark/regexp2"
)

type file struct {
	Normalizer  json.RawMessage `json:"normalizer"`
	AddedTokens []struct {
		ID         int    `json:"id"`
		Content    string `json:"content"`
		SingleWord bool   `json:"single_word"`
		LStrip     bool   `json:"lstrip"`
		RStrip     bool   `json:"rstrip"`
		Normalized bool   `json:"normalized"`
	} `json:"added_tokens"`
	PreTokenizer struct {
		Type          string `json:"type"`
		PreTokenizers []struct {
			Type    string `json:"type"`
			Pattern struct {
				Regex string `json:"Regex"`
			} `json:"pattern"`
			Behavior       string `json:"behavior"`
			Invert         bool   `json:"invert"`
			AddPrefixSpace bool   `json:"add_prefix_space"`
			UseRegex       bool   `json:"use_regex"`
		} `json:"pretokenizers"`
	} `json:"pre_tokenizer"`
	PostProcessor struct {
		Type   string `json:"type"`
		Single []struct {
			SpecialToken *struct {
				ID     string `json:"id"`
				TypeID int    `json:"type_id"`
			} `json:"SpecialToken"`
			Sequence *struct {
				ID     string `json:"id"`
				TypeID int    `json:"type_id"`
			} `json:"Sequence"`
		} `json:"single"`
		SpecialTokens map[string]struct {
			IDs []int `json:"ids"`
		} `json:"special_tokens"`
	} `json:"post_processor"`
	Model struct {
		Type                    string          `json:"type"`
		Dropout                 *float64        `json:"dropout"`
		UnkToken                *string         `json:"unk_token"`
		ContinuingSubwordPrefix *string         `json:"continuing_subword_prefix"`
		EndOfWordSuffix         *string         `json:"end_of_word_suffix"`
		ByteFallback            bool            `json:"byte_fallback"`
		IgnoreMerges            bool            `json:"ignore_merges"`
		Vocab                   map[string]int  `json:"vocab"`
		Merges                  json.RawMessage `json:"merges"`
	} `json:"model"`
}

type pair struct{ left, right int }

type merge struct{ rank, id int }

type added struct {
	content    string
	id         int
	singleWord bool
}

// Tokenizer encodes text without truncation or padding. It is safe for
// concurrent use.
type Tokenizer struct {
	split        *regexp2.Regexp
	vocab        map[string]int
	merges       map[pair]merge
	ignoreMerges bool
	added        []added
	prefix       []int // special token IDs before the sequence
	suffix       []int // special token IDs after the sequence
}

// Encoding holds token IDs and byte offsets into the original text.
type Encoding struct {
	IDs     []int
	Offsets [][2]int
}

// byteLevel is the GPT-2 byte alphabet: each byte becomes one printable rune.
var byteLevel = func() (table [256]string) {
	next := 256
	for b := range 256 {
		if b >= '!' && b <= '~' || b >= 0xA1 && b <= 0xAC || b >= 0xAE {
			table[b] = string(rune(b))
		} else {
			table[b] = string(rune(next))
			next++
		}
	}
	return table
}()

// Load reads a tokenizer.json file.
func Load(path string) (*Tokenizer, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	var f file
	if err := json.Unmarshal(data, &f); err != nil {
		return nil, err
	}
	unsupported := func(what string) (*Tokenizer, error) {
		return nil, fmt.Errorf("tokenizer %s is not supported", what)
	}
	if len(f.Normalizer) != 0 && string(f.Normalizer) != "null" {
		return unsupported("normalizer")
	}
	stages := f.PreTokenizer.PreTokenizers
	if f.PreTokenizer.Type != "Sequence" || len(stages) != 2 ||
		stages[0].Type != "Split" || stages[0].Behavior != "Isolated" || stages[0].Invert || stages[0].Pattern.Regex == "" ||
		stages[1].Type != "ByteLevel" || stages[1].AddPrefixSpace || stages[1].UseRegex {
		return unsupported("pre-tokenizer")
	}
	model := f.Model
	if model.Type != "BPE" || model.Dropout != nil || model.UnkToken != nil || model.ContinuingSubwordPrefix != nil ||
		model.EndOfWordSuffix != nil || model.ByteFallback {
		return unsupported("model")
	}
	split, err := regexp2.Compile(stages[0].Pattern.Regex, regexp2.None)
	if err != nil {
		return nil, fmt.Errorf("tokenizer split pattern is invalid: %w", err)
	}
	t := &Tokenizer{split: split, vocab: model.Vocab, merges: map[pair]merge{}, ignoreMerges: model.IgnoreMerges}
	// Merges are either "a b" strings or [a, b] pairs.
	var legacy []string
	var tuples [][2]string
	if err := json.Unmarshal(model.Merges, &tuples); err != nil {
		if err := json.Unmarshal(model.Merges, &legacy); err != nil {
			return nil, errors.New("tokenizer merges are invalid")
		}
		tuples = nil
		for _, line := range legacy {
			left, right, ok := strings.Cut(line, " ")
			if !ok {
				return nil, errors.New("tokenizer merges are invalid")
			}
			tuples = append(tuples, [2]string{left, right})
		}
	}
	for rank, tuple := range tuples {
		left, okLeft := t.vocab[tuple[0]]
		right, okRight := t.vocab[tuple[1]]
		id, okMerged := t.vocab[tuple[0]+tuple[1]]
		if !okLeft || !okRight || !okMerged {
			return nil, errors.New("tokenizer merge is missing from the vocabulary")
		}
		t.merges[pair{left, right}] = merge{rank, id}
	}
	for _, token := range f.AddedTokens {
		if token.LStrip || token.RStrip || token.Normalized || token.Content == "" {
			return unsupported("added token option")
		}
		t.added = append(t.added, added{token.Content, token.ID, token.SingleWord})
	}
	if f.PostProcessor.Type != "TemplateProcessing" {
		return unsupported("post-processor")
	}
	seen := false
	for _, piece := range f.PostProcessor.Single {
		switch {
		case piece.Sequence != nil && piece.Sequence.ID == "A" && piece.Sequence.TypeID == 0 && !seen:
			seen = true
		case piece.SpecialToken != nil && piece.SpecialToken.TypeID == 0:
			ids := f.PostProcessor.SpecialTokens[piece.SpecialToken.ID].IDs
			if len(ids) == 0 {
				return unsupported("post-processor template")
			}
			if seen {
				t.suffix = append(t.suffix, ids...)
			} else {
				t.prefix = append(t.prefix, ids...)
			}
		default:
			return unsupported("post-processor template")
		}
	}
	if !seen {
		return unsupported("post-processor template")
	}
	return t, nil
}

// Encode returns token IDs and original byte offsets. Special tokens have empty offsets.
func (t *Tokenizer) Encode(text string, special bool) (Encoding, error) {
	var out Encoding
	if special {
		for _, id := range t.prefix {
			out.IDs, out.Offsets = append(out.IDs, id), append(out.Offsets, [2]int{})
		}
	}
	position := 0
	for position < len(text) {
		// The leftmost added token wins, the longest among those starting together.
		start, end, id := len(text), len(text), -1
		for _, token := range t.added {
			index := t.find(text, position, token)
			if index < 0 {
				continue
			}
			if index < start || index == start && index+len(token.content) > end {
				start, end, id = index, index+len(token.content), token.id
			}
		}
		if err := t.segment(text, position, start, &out); err != nil {
			return Encoding{}, err
		}
		if id >= 0 {
			out.IDs, out.Offsets = append(out.IDs, id), append(out.Offsets, [2]int{start, end})
		}
		position = end
	}
	if special {
		for _, id := range t.suffix {
			out.IDs, out.Offsets = append(out.IDs, id), append(out.Offsets, [2]int{})
		}
	}
	return out, nil
}

// find returns the first acceptable occurrence of an added token at or after position.
// A single-word token must not touch a word character on either side.
func (t *Tokenizer) find(text string, position int, token added) int {
	for position <= len(text) {
		index := strings.Index(text[position:], token.content)
		if index < 0 {
			return -1
		}
		index += position
		if !token.singleWord {
			return index
		}
		before, _ := utf8.DecodeLastRuneInString(text[:index])
		after, _ := utf8.DecodeRuneInString(text[index+len(token.content):])
		if (index == 0 || !wordRune(before)) && (index+len(token.content) == len(text) || !wordRune(after)) {
			return index
		}
		position = index + len(token.content)
	}
	return -1
}

// wordRune is the Unicode \w class that bounds single-word added tokens.
func wordRune(r rune) bool {
	return unicode.IsLetter(r) || unicode.IsMark(r) || unicode.Is(unicode.Nd, r) || unicode.Is(unicode.Pc, r) ||
		unicode.Is(unicode.Other_Alphabetic, r) || r == 0x200C || r == 0x200D
}

// segment pre-tokenizes text[start:end] and appends its BPE tokens.
func (t *Tokenizer) segment(text string, start, end int, out *Encoding) error {
	if start >= end {
		return nil
	}
	part := text[start:end]
	runes := []rune(part)
	// regexp2 reports rune indices; bytes[i] is the byte offset of rune i.
	bytes := make([]int, len(runes)+1)
	offset := 0
	for index, r := range runes {
		bytes[index] = offset
		offset += utf8.RuneLen(r)
	}
	bytes[len(runes)] = offset
	if offset != len(part) {
		return errors.New("tokenizer input is not valid UTF-8")
	}
	cursor := 0
	match, err := t.split.FindRunesMatch(runes)
	for ; err == nil && match != nil; match, err = t.split.FindNextMatch(match) {
		if match.Length == 0 {
			return errors.New("tokenizer split pattern matched an empty span")
		}
		// Isolated: both the gap before a match and the match are words.
		t.word(part, start, bytes[cursor], bytes[match.Index], out)
		t.word(part, start, bytes[match.Index], bytes[match.Index+match.Length], out)
		cursor = match.Index + match.Length
	}
	if err != nil {
		return err
	}
	t.word(part, start, bytes[cursor], len(part), out)
	return nil
}

// word applies byte-level BPE to part[from:to]; base is the offset of part in the input.
func (t *Tokenizer) word(part string, base, from, to int, out *Encoding) {
	if from >= to {
		return
	}
	var builder strings.Builder
	for index := from; index < to; index++ {
		builder.WriteString(byteLevel[part[index]])
	}
	if id, ok := t.vocab[builder.String()]; ok && t.ignoreMerges {
		out.IDs, out.Offsets = append(out.IDs, id), append(out.Offsets, [2]int{base + from, base + to})
		return
	}
	// One symbol per input byte. A byte missing from the vocabulary is dropped
	// (there is no unknown token) and takes no position: the symbols after it
	// report offsets one byte earlier, as the reference tokenizer does.
	ids := make([]int, 0, to-from)
	spans := make([][2]int, 0, to-from)
	for index := from; index < to; index++ {
		if id, known := t.vocab[byteLevel[part[index]]]; known {
			position := base + from + len(ids)
			ids, spans = append(ids, id), append(spans, [2]int{position, position + 1})
		}
	}
	for {
		// Lowest rank first, leftmost among equals.
		best, at := merge{rank: -1}, -1
		for index := 0; index+1 < len(ids); index++ {
			if candidate, ok := t.merges[pair{ids[index], ids[index+1]}]; ok && (at < 0 || candidate.rank < best.rank) {
				best, at = candidate, index
			}
		}
		if at < 0 {
			break
		}
		ids[at], spans[at][1] = best.id, spans[at+1][1]
		ids, spans = append(ids[:at+1], ids[at+2:]...), append(spans[:at+1], spans[at+2:]...)
	}
	// Offsets follow original characters: a token that starts or ends inside
	// a multi-byte character reports that whole character.
	for index := range spans {
		for spans[index][0] > base && !utf8.RuneStart(part[spans[index][0]-base]) {
			spans[index][0]--
		}
		for spans[index][1] < base+len(part) && !utf8.RuneStart(part[spans[index][1]-base]) {
			spans[index][1]++
		}
	}
	out.IDs, out.Offsets = append(out.IDs, ids...), append(out.Offsets, spans...)
}
