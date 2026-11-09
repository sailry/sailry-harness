package i18n

import (
	"bufio"
	"os"
	"strings"
	"testing"
)

// Every key listed is in every catalog, and the catalogs hold nothing else.
func TestCatalogsComplete(t *testing.T) {
	f, err := os.Open("keys.txt")
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	want := map[string]bool{}
	s := bufio.NewScanner(f)
	for s.Scan() {
		if k := strings.TrimSpace(s.Text()); k != "" && !strings.HasPrefix(k, "#") {
			want[k] = true
		}
	}
	for _, lang := range Languages {
		keys := Keys(lang)
		if len(keys) != len(want) {
			t.Errorf("%s has %d keys, keys.txt %d", lang, len(keys), len(want))
		}
		for _, k := range keys {
			if !want[k] {
				t.Errorf("%s: %s is not in keys.txt", lang, k)
			}
		}
	}
}

func TestPlaceholdersAndMatch(t *testing.T) {
	if got := In("en", "provider_model_count", "count", 3); got != "3 models" {
		t.Errorf("en count: %q", got)
	}
	if got := In("zh-CN", "sidebar_age_minutes", "count", 5); got != "5m" {
		t.Errorf("zh-CN age: %q", got)
	}
	for tag, want := range map[string]string{"zh-Hans-CN": "zh-CN", "zh-CN": "zh-CN", "zh-TW": "en", "en-GB": "en", "de": "en"} {
		if got := Match(tag); got != want {
			t.Errorf("Match(%q) = %q, want %q", tag, got, want)
		}
	}
}
