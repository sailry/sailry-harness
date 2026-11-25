package composer

import (
	"slices"
	"strings"
	"testing"
	"unicode"
)

// runeTrigger is parseTrigger over a rune copy of the draft, the
// specification the in-place scan keeps.
func runeTrigger(text string, caret int) trigger {
	rs := []rune(text)
	if caret < 0 || caret > len(rs) {
		return trigger{}
	}
	for i := caret - 1; i >= 0; i-- {
		r := rs[i]
		if r != '@' && r != '/' {
			if unicode.IsSpace(r) {
				return trigger{}
			}
			continue
		}
		if i > 0 && !unicode.IsSpace(rs[i-1]) || caret < len(rs) && !unicode.IsSpace(rs[caret]) {
			return trigger{}
		}
		query := string(rs[i+1 : caret])
		if r == '/' && strings.Contains(query, "/") {
			return trigger{}
		}
		return trigger{start: i, marker: r, query: query, ok: true}
	}
	return trigger{}
}

func runeChipRanges(draft string, references []Reference) [][2]int {
	var ranges [][2]int
	rs := []rune(draft)
	for _, r := range references {
		t := []rune(token(r))
		for i := 0; i+len(t) <= len(rs); i++ {
			if slices.Equal(rs[i:i+len(t)], t) {
				ranges = append(ranges, [2]int{i, i + len(t)})
				i += len(t) - 1
			}
		}
	}
	return ranges
}

var referenceDrafts = []string{
	"", "@", "/", " @", "@é", "a@b", "héllo @wörld", "/compact focus", "x /a/b", "日本 /コマンド", "@@文件 @文件 ",
	"tab\t@q", "line\n/cmd", "@文件@文件", "a @ b", "/ ", "emoji 🙂 @🙂 /🙂",
}

func TestParseTriggerMatchesRuneScan(t *testing.T) {
	for _, draft := range referenceDrafts {
		for caret := -1; caret <= len([]rune(draft))+1; caret++ {
			if got, want := parseTrigger(draft, caret), runeTrigger(draft, caret); got != want {
				t.Errorf("parseTrigger(%q, %d) = %+v, want %+v", draft, caret, got, want)
			}
		}
	}
}

func TestChipRangesMatchRuneScan(t *testing.T) {
	references := []Reference{{ID: "1", Label: "文件"}, {ID: "2", Label: "@🙂"}, {ID: "3", Label: "wörld"}, {ID: "4", Label: ""}}
	for _, draft := range referenceDrafts {
		s := &State{Draft: draft, References: references}
		var got [][2]int
		s.chipRanges(func(start, end int) { got = append(got, [2]int{start, end}) })
		if want := runeChipRanges(draft, references); !slices.Equal(got, want) {
			t.Errorf("chip ranges of %q = %v, want %v", draft, got, want)
		}
		for _, r := range references {
			if s.chipped(r) != strings.Contains(draft, token(r)) {
				t.Errorf("chipped(%q) in %q disagrees with the token", r.Label, draft)
			}
		}
	}
	s := &State{Draft: "keep @文件 ", References: []Reference{{ID: "1", Label: "文件"}, {ID: "2", Label: "gone"}}}
	s.syncReferences()
	if len(s.References) != 1 || s.References[0].ID != "1" {
		t.Fatalf("syncReferences kept %+v", s.References)
	}
	kept := s.References
	s.syncReferences()
	if &kept[0] != &s.References[0] {
		t.Fatal("syncReferences rebuilt references whose chips are all present")
	}
}

// BenchmarkReferenceFrame is the composer's per-frame reference work on a
// long draft with chips: the trigger at the caret, the chip check and ranges.
func BenchmarkReferenceFrame(b *testing.B) {
	draft := strings.Repeat("长草稿 draft text with words ", 400) + "@文件 @wörld /comp"
	s := &State{Draft: draft, References: []Reference{{ID: "1", Label: "文件"}, {ID: "2", Label: "wörld"}}}
	caret := len([]rune(draft))
	ranges := 0
	b.ReportAllocs()
	for b.Loop() {
		if !parseTrigger(s.Draft, caret).ok {
			b.Fatal("the trigger at the caret was not found")
		}
		s.syncReferences()
		if !s.hasInput() {
			b.Fatal("the draft has no input")
		}
		s.chipRanges(func(int, int) { ranges++ })
	}
	if ranges == 0 {
		b.Fatal("no chips")
	}
}
