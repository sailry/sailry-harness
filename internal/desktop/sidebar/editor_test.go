package sidebar

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// TestEditorChecks groups the project editor's pure checks; each case keeps its own fixture.
func TestEditorChecks(t *testing.T) {
	t.Run("SuggestName", testSuggestName)
	t.Run("CheckProject", testCheckProject)
	t.Run("CheckClone", testCheckClone)
	t.Run("SampleTree", testSampleTree)
}

func testSuggestName(t *testing.T) {
	for in, want := range map[string]string{
		"~/Web/sailry-go":                       "sailry-go",
		"~/Web/sailry-go/":                      "sailry-go",
		`C:\Work\billing`:                       "billing",
		"https://github.com/org/repository.git": "repository",
		"git@github.com:org/tool.git":           "tool",
	} {
		if got := SuggestName(in, true); got != want {
			t.Errorf("SuggestName(%q) = %q, want %q", in, got, want)
		}
	}
	// Only a repository loses ".git": a folder keeps its name.
	if got := SuggestName("~/Work/mirror.git/ ", false); got != "mirror.git" {
		t.Errorf("SuggestName of a folder = %q", got)
	}
}

func testCheckProject(t *testing.T) {
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

func testCheckClone(t *testing.T) {
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

// The preview's sample tree holds the host's sample projects under the
// sample account's home, and the files the right panel shows under their
// project, so a picker opens on what the window names.
func testSampleTree(t *testing.T) {
	s := New(preview.Load(), preview.Big)
	catalog := s.Catalog("local")
	if catalog == nil || catalog != s.Catalog("local") {
		t.Fatal("the preview keeps no sample tree for its host")
	}
	listing, problem := catalog.List("/Users/alex/Web/sailry-go")
	if problem != "" {
		t.Fatalf("the sample project is not in the tree: %s", problem)
	}
	var names []string
	for _, entry := range listing.Entries {
		names = append(names, entry.Name+":"+entry.Kind)
	}
	if want := []string{"README.md:file", "docs:directory", "go.mod:file", "internal:directory"}; !slices.Equal(names, want) {
		t.Fatalf("the sample project lists %q, want %q", names, want)
	}
	if catalog.Size("/Users/alex/Web/sailry-go/go.mod") == 0 || catalog.Size("/Users/alex/Web/sailry-go/docs") != 0 {
		t.Fatal("a sample file has no size, or a directory has one")
	}
	// A project of another host is not in this host's tree.
	if _, problem := catalog.List("/Users/alex/Work/billing-api"); problem == "" {
		t.Fatal("the remote host's project is in the local tree")
	}
	var places []string
	for _, place := range s.projectPlaces("local") {
		places = append(places, place.Path)
	}
	if !slices.Contains(places, "/Users/alex/Web/sailry-go") || slices.Contains(places, "~/Web/sailry-go") {
		t.Fatalf("the picker's projects are %q", places)
	}
	s.Actions = &Actions{}
	if s.Catalog("local") != nil {
		t.Fatal("a live window has a sample tree")
	}
}
