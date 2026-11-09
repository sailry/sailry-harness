package sidebar

import (
	"testing"

	"github.com/sailry/sailry-go/internal/preview"
)

func TestSuggestName(t *testing.T) {
	for in, want := range map[string]string{
		"~/Web/sailry-go":                       "sailry-go",
		"~/Web/sailry-go/":                      "sailry-go",
		`C:\Work\billing`:                       "billing",
		"https://github.com/org/repository.git": "repository",
		"git@github.com:org/tool.git":           "tool",
	} {
		if got := SuggestName(in); got != want {
			t.Errorf("SuggestName(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestCheckProject(t *testing.T) {
	a := &preview.Project{Path: "~/Web/a", Host: "local"}
	others := []*preview.Project{a}
	if k := CheckProject("", "~/x", "local", others, nil); k != "project_required" {
		t.Errorf("no name: %q", k)
	}
	if k := CheckProject("b", "~/Web/a/", "local", others, nil); k != "project_duplicate_path" {
		t.Errorf("same path: %q", k)
	}
	if k := CheckProject("b", "~/Web/a", "remote", others, nil); k != "" {
		t.Errorf("another host: %q", k)
	}
	if k := CheckProject("a", "~/Web/a", "local", others, a); k != "" {
		t.Errorf("editing itself: %q", k)
	}
}

func TestCheckClone(t *testing.T) {
	if k := CheckClone("tool", "https://x/tool.git", "~/src"); k != "" {
		t.Errorf("valid clone: %q", k)
	}
	for _, name := range []string{"", ".", "..", "a/b", "a:b"} {
		if CheckClone(name, "r", "p") == "" {
			t.Errorf("name %q passed", name)
		}
	}
	if got := Destination(`C:\src\`, "tool"); got != `C:\src\tool` {
		t.Errorf("destination %q", got)
	}
}
