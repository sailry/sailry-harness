package picker

import (
	"slices"
	"testing"
)

// TestModel covers the picker's rules without a window: what the hidden
// toggle and the filter keep, folders first in every order, what Confirm
// chooses, the address's crumbs on both separators, kinds, sizes, names and
// the preview's in-memory directories.
func TestModel(t *testing.T) {
	entries := []Entry{
		{Name: "zeta.txt", Kind: "file", Size: 2000},
		{Name: ".git", Kind: "directory"},
		{Name: "Beta", Kind: "directory"},
		{Name: "alpha.md", Kind: "file", Size: 12},
		{Name: "alpha", Kind: "directory"},
	}
	names := func(indexes []int) (out []string) {
		for _, i := range indexes {
			out = append(out, entries[i].Name)
		}
		return
	}
	kind := func(e Entry) string { key, extension := KindKey(e); return key + extension }

	if got := names(Visible(entries, false, "")); !slices.Equal(got, []string{"zeta.txt", "Beta", "alpha.md", "alpha"}) {
		t.Errorf("without hidden entries: %q", got)
	}
	if got := names(Visible(entries, true, "ALPHA")); !slices.Equal(got, []string{"alpha.md", "alpha"}) {
		t.Errorf("the filter ignores case: %q", got)
	}
	if got := Visible(entries, false, "git"); len(got) != 0 {
		t.Errorf("the filter showed a hidden entry: %q", names(got))
	}
	shown := Visible(entries, true, "")
	Order(entries, shown, "name", false, kind)
	if got := names(shown); !slices.Equal(got, []string{".git", "alpha", "Beta", "alpha.md", "zeta.txt"}) {
		t.Errorf("by name: %q", got)
	}
	Order(entries, shown, "name", true, kind)
	if got := names(shown); !slices.Equal(got, []string{"Beta", "alpha", ".git", "zeta.txt", "alpha.md"}) {
		t.Errorf("by name descending keeps folders first: %q", got)
	}
	Order(entries, shown, "size", true, kind)
	if got := names(shown)[3:]; !slices.Equal(got, []string{"zeta.txt", "alpha.md"}) {
		t.Errorf("by size descending: %q", got)
	}

	listing := Listing{Separator: "/", Path: "/work", Parent: "/", Entries: entries}
	for _, c := range []struct {
		mode     Mode
		selected []int
		want     []string
	}{
		{Directory, nil, []string{"/work"}},
		{Directory, []int{2}, []string{"/work/Beta"}},
		{Directory, []int{0}, nil},
		{Directory, []int{2, 4}, nil},
		{File, []int{3}, []string{"/work/alpha.md"}},
		{File, []int{2}, nil},
		{File, nil, nil},
	} {
		if got := Resolve(c.mode, listing, c.selected); !slices.Equal(got, c.want) {
			t.Errorf("Resolve(%v, %v) = %q, want %q", c.mode, c.selected, got, c.want)
		}
	}

	for _, c := range []struct {
		listing Listing
		want    []Crumb
	}{
		{Listing{Separator: "/", Path: "/"}, []Crumb{{"/", "/"}}},
		{Listing{Separator: "/", Path: "/home/me"}, []Crumb{{"/", "/"}, {"home", "/home"}, {"me", "/home/me"}}},
		{Listing{Separator: "/", Path: "/Volumes/Data/src", Locations: []Location{{"root", "", "/"}, {"volume", "Data", "/Volumes/Data"}}},
			[]Crumb{{"Data", "/Volumes/Data"}, {"src", "/Volumes/Data/src"}}},
		{Listing{Separator: `\`, Path: `D:\source\目录`}, []Crumb{{"D:", `D:\`}, {"source", `D:\source`}, {"目录", `D:\source\目录`}}},
		{Listing{Separator: `\`, Path: `\\node\share\folder`}, []Crumb{{`\\node\share`, `\\node\share`}, {"folder", `\\node\share\folder`}}},
	} {
		if got := Crumbs(c.listing); !slices.Equal(got, c.want) {
			t.Errorf("Crumbs(%q) = %q, want %q", c.listing.Path, got, c.want)
		}
	}

	for name, want := range map[string]string{"a.md": "directory_markdown_kind", "a.YML": "directory_yaml_kind", "a.png": "directory_image_kind",
		"Makefile": "directory_file_kind", "a.go": "directory_extension_kindGO"} {
		if got := kind(Entry{Name: name, Kind: "file"}); got != want {
			t.Errorf("the kind of %s is %s, want %s", name, got, want)
		}
	}
	if got := kind(Entry{Name: "a.md", Kind: "directory"}); got != "directory_folder_kind" {
		t.Errorf("a directory's kind is %s", got)
	}
	for size, want := range map[uint64]string{0: "0 B", 999: "999 B", 1000: "1.0 KB", 1_500_000: "1.5 MB"} {
		if got := Bytes(Entry{Kind: "file", Size: size}); got != want {
			t.Errorf("Bytes(%d) = %q, want %q", size, got, want)
		}
	}
	if got := Bytes(Entry{Kind: "directory", Size: 4096}); got != "—" {
		t.Errorf("a directory's size shows %q", got)
	}
	for name, want := range map[string]bool{"notes": true, "": false, " ": false, ".": false, "..": false, "a/b": false, `a\b`: false} {
		if ValidName(name) != want {
			t.Errorf("ValidName(%q) = %v", name, !want)
		}
	}

	// The preview's directories: listed, refused, missing, and changed in
	// memory alone.
	catalog := NewCatalog(false)
	var home Listing
	var problem string
	catalog.Source().Browse("", func(l Listing, p string) { home, problem = l, p })
	if problem != "" || home.Path != "/preview" || len(home.Entries) < 4 {
		t.Fatalf("the preview's home is %+v: %s", home, problem)
	}
	if _, problem := catalog.List("/preview/restricted"); problem != "directory_denied" {
		t.Errorf("a restricted directory answers %q", problem)
	}
	if _, problem := catalog.List("/preview/nowhere"); problem != "directory_missing" {
		t.Errorf("a missing directory answers %q", problem)
	}
	if problem := catalog.Apply(Action{Kind: "create", Path: "/preview/empty/new"}); problem != "" {
		t.Fatal("creating a folder failed:", problem)
	}
	if problem := catalog.Apply(Action{Kind: "rename", Path: "/preview/empty/new", To: "/preview/empty/renamed"}); problem != "" {
		t.Fatal("renaming a folder failed:", problem)
	}
	if empty, _ := catalog.List("/preview/empty"); len(empty.Entries) != 1 || empty.Entries[0].Name != "renamed" {
		t.Errorf("after creating and renaming: %+v", empty.Entries)
	}
	if problem := catalog.Apply(Action{Kind: "trash", Paths: []string{"/preview/empty/renamed"}}); problem != "" {
		t.Fatal("trashing a folder failed:", problem)
	}
	if empty, _ := catalog.List("/preview/empty"); len(empty.Entries) != 0 {
		t.Errorf("after trashing: %+v", empty.Entries)
	}

	t.Run("tile classes", func(t *testing.T) {
		for name, want := range map[string]string{
			"main.go": "code", "notes.md": "text", "paper.PDF": "pdf", "photo.jpeg": "image", "data.csv": "sheet",
			"backup.tar": "archive", "song.mp3": "audio", "clip.mov": "video", "Makefile": "file", ".gitignore": "file",
		} {
			if got := Class(Entry{Name: name, Kind: "file"}); got != want {
				t.Errorf("Class(%q) = %q, want %q", name, got, want)
			}
		}
		if got := Class(Entry{Name: "archive.zip", Kind: "directory"}); got != "folder" {
			t.Errorf("a directory's class is %q", got)
		}
	})

	t.Run("several files", func(t *testing.T) {
		files := Listing{Separator: "/", Path: "/work", Entries: []Entry{
			{Name: "a.go", Kind: "file"}, {Name: "b.md", Kind: "file"}, {Name: "src", Kind: "directory"},
		}}
		var picked []string
		s := &State{}
		s.Open(Options{Mode: Files,
			Source: Source{Browse: func(_ string, done func(Listing, string)) { done(files, "") }},
			Choose: func(paths []string) bool { picked = paths; return true }})
		if got := s.selection(); got != nil {
			t.Errorf("Files mode chooses %q with nothing chosen", got)
		}
		for _, name := range []string{"a.go", "b.md", "src"} {
			s.chosen.Add(name)
		}
		if got := s.selection(); !slices.Equal(got, []string{"/work/a.go", "/work/b.md"}) {
			t.Errorf("the chosen files are %q", got)
		}
		// A double click or Enter on a file chooses it alone and confirms.
		s.activate(1)
		if !slices.Equal(picked, []string{"/work/b.md"}) || s.Showing() {
			t.Errorf("activating a file chose %q, showing %v", picked, s.Showing())
		}
	})
}
