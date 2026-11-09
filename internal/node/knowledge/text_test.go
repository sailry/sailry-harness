package knowledge

import (
	"strings"
	"testing"
)

func TestCJKSplit(t *testing.T) {
	t.Parallel()
	for _, item := range []struct {
		operation   func(string) string
		input, want string
	}{
		{Field, "Café C++ C# RUST", "cafe c++ c# rust"},
		{Field, "A, B\nC", "a b c"},
		{Field, "Cafe\u0301", "cafe"},
		{Index, "ASCII café C++", ""},
		{Index, "AI向量", "ai 向 量 向量"},
		{Index, "向", "向"},
		{Phrase, "AI向量", `"ai 向 量"`},
		{Phrase, "title:literal OR secret*", `"title literal or secret"`},
		{Phrase, `"quoted"`, `"quoted"`},
		{Phrase, `missing" OR "secretive`, `"missing or secretive"`},
		{Phrase, `" * : ()`, ""},
		{Query, "rust async cancellation", `"async" OR "cancellation" OR "rust"`},
		{Query, "AI向量数据库", `"ai" OR "向量" OR "据库" OR "数据" OR "量数"`},
		{Query, "向", `"向"`},
		{Query, "rust rust", `"rust"`},
		{Query, "C++ C#", `"c#" OR "c++"`},
		{Query, "Cafe\u0301", `"cafe"`},
		{Query, `" ( ) * :`, ""},
		{Query, "title:literal OR secret*", `"literal" OR "or" OR "secret" OR "title"`},
	} {
		if got := item.operation(item.input); got != item.want {
			t.Errorf("input %q: got %q, want %q", item.input, got, item.want)
		}
	}
	for _, term := range []string{"向", "量", "数", "据", "库", "向量", "量数", "数据", "据库"} {
		if !strings.Contains(" "+Index("向量数据库")+" ", " "+term+" ") {
			t.Errorf("missing CJK term %q", term)
		}
	}
	for _, text := range []string{"漢字", "すし", "カナ", "한글", "𠀀𠀁"} {
		if !strings.Contains(Index(text), Field(text)) {
			t.Errorf("missing script pair %q", text)
		}
	}
	for _, punctuation := range "・。" {
		if cjk(punctuation) {
			t.Errorf("punctuation %q is a CJK word", punctuation)
		}
	}
}
