package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// A language's catalogs are merged, and a key defined twice among them is an
// error: in two files, with the same value or another, or twice in one file.
func TestReadLang(t *testing.T) {
	write := func(t *testing.T, files map[string]string) string {
		t.Helper()
		dir := t.TempDir()
		for name, text := range files {
			if err := os.WriteFile(filepath.Join(dir, name), []byte(text), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		return dir
	}
	t.Run("merged", func(t *testing.T) {
		dir := write(t, map[string]string{
			"en.yml": "save: Save\n", "git.en.yml": "git_clone: \"Clone…\"\n", "zz-desktop.en.yml": "blur: Blur\n",
			// Another language's catalogs are its own: the same keys there are no duplicate.
			"zh-CN.yml": "save: 保存\n", "git.zh-CN.yml": "git_clone: 克隆…\n",
		})
		for lang, want := range map[string]map[string]string{
			"en":    {"save": "Save", "git_clone": "Clone…", "blur": "Blur"},
			"zh-CN": {"save": "保存", "git_clone": "克隆…"},
		} {
			got, err := readLang(dir, lang)
			if err != nil {
				t.Fatal(err)
			}
			if len(got) != len(want) {
				t.Errorf("%s: %v, want %v", lang, got, want)
			}
			for k, v := range want {
				if got[k] != v {
					t.Errorf("%s: %s is %q, want %q", lang, k, got[k], v)
				}
			}
		}
	})
	for name, files := range map[string]map[string]string{
		"another value in a later file": {"git.en.yml": "git_clone: Clone\n", "zz-desktop.en.yml": "git_clone: \"Clone…\"\n"},
		"the same value in two files":   {"en.yml": "git_clone: Clone\n", "git.en.yml": "git_clone: Clone\n"},
	} {
		t.Run(name, func(t *testing.T) {
			_, err := readLang(write(t, files), "en")
			if err == nil {
				t.Fatal("a key defined in two files was merged")
			}
			for _, part := range []string{"git_clone", "git.en.yml"} {
				if !strings.Contains(err.Error(), part) {
					t.Errorf("the error does not name %q: %v", part, err)
				}
			}
		})
	}
	t.Run("twice in one file", func(t *testing.T) {
		if _, err := readLang(write(t, map[string]string{"en.yml": "save: Save\nsave: Keep\n"}), "en"); err == nil {
			t.Fatal("a key repeated in one file was merged")
		}
	})
}
