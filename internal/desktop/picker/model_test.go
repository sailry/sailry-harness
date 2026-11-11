package picker

import (
	"slices"
	"testing"
	"time"
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
		{Listing{Separator: "/", Path: "/Volumes/Data/src", Locations: []Location{{Kind: "root", Path: "/"}, {Kind: "volume", Name: "Data", Path: "/Volumes/Data"}}},
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
	if problem != "" || home.Path != "/Users/alex" || len(home.Entries) < 4 {
		t.Fatalf("the preview's home is %+v: %s", home, problem)
	}
	if _, problem := catalog.List("/Users/alex/restricted"); problem != "directory_denied" {
		t.Errorf("a restricted directory answers %q", problem)
	}
	if _, problem := catalog.List("/Users/alex/nowhere"); problem != "directory_missing" {
		t.Errorf("a missing directory answers %q", problem)
	}
	if problem := catalog.Apply(Action{Kind: "create", Path: "/Users/alex/empty/new"}); problem != "" {
		t.Fatal("creating a folder failed:", problem)
	}
	if problem := catalog.Apply(Action{Kind: "rename", Path: "/Users/alex/empty/new", To: "/Users/alex/empty/renamed"}); problem != "" {
		t.Fatal("renaming a folder failed:", problem)
	}
	if empty, _ := catalog.List("/Users/alex/empty"); len(empty.Entries) != 1 || empty.Entries[0].Name != "renamed" {
		t.Errorf("after creating and renaming: %+v", empty.Entries)
	}
	if problem := catalog.Apply(Action{Kind: "trash", Paths: []string{"/Users/alex/empty/renamed"}}); problem != "" {
		t.Fatal("trashing a folder failed:", problem)
	}
	if empty, _ := catalog.List("/Users/alex/empty"); len(empty.Entries) != 0 {
		t.Errorf("after trashing: %+v", empty.Entries)
	}

	// The Modified column orders by time inside the folders and the files,
	// an entry without one first; today's and yesterday's times are told
	// from the dates in the reader's time zone.
	t.Run("modified", func(t *testing.T) {
		now := time.Date(2026, 11, 11, 15, 0, 0, 0, time.UTC)
		dated := []Entry{
			{Name: "old.txt", Kind: "file", Modified: now.Add(-72 * time.Hour)},
			{Name: "undated.txt", Kind: "file"},
			{Name: "new.txt", Kind: "file", Modified: now.Add(-time.Hour)},
			{Name: "folder", Kind: "directory", Modified: now},
			{Name: "archive", Kind: "directory", Modified: now.Add(-24 * time.Hour)},
		}
		order := func(descending bool) (out []string) {
			shown := Visible(dated, true, "")
			Order(dated, shown, "modified", descending, kind)
			for _, i := range shown {
				out = append(out, dated[i].Name)
			}
			return
		}
		if got := order(false); !slices.Equal(got, []string{"archive", "folder", "undated.txt", "old.txt", "new.txt"}) {
			t.Errorf("by modified: %q", got)
		}
		if got := order(true); !slices.Equal(got, []string{"folder", "archive", "new.txt", "old.txt", "undated.txt"}) {
			t.Errorf("by modified descending: %q", got)
		}
		east := time.FixedZone("east", 8*3600)
		for _, c := range []struct {
			modified, now time.Time
			want          int
		}{
			{time.Time{}, now, -1},
			{now, now, 0},
			{now.Add(-15 * time.Hour), now, 0},
			{now.Add(-16 * time.Hour), now, 1},
			{now.Add(-40 * time.Hour), now, 2},
			{now.Add(10 * time.Hour), now, 2},
			// 23:00 the day before in UTC is 07:00 today eight hours east.
			{now.Add(-16 * time.Hour), now.In(east), 0},
		} {
			if got := Day(c.modified, c.now); got != c.want {
				t.Errorf("Day(%v, %v) = %d, want %d", c.modified, c.now, got, c.want)
			}
		}
		// The preview's files carry sizes and times against its own clock.
		sample := slices.IndexFunc(home.Entries, func(e Entry) bool { return e.Name == "release-notes.md" })
		if sample < 0 || home.Entries[sample].Kind != "file" || home.Entries[sample].Size != 6200 || Day(home.Entries[sample].Modified, CatalogNow) != 0 {
			t.Errorf("the preview's root lists %+v", home.Entries)
		}
	})

	// A directory picker selects directories alone and always has a choice;
	// the file pickers select anything and choose files.
	t.Run("selection rules", func(t *testing.T) {
		l := Listing{Separator: "/", Path: "/work", Entries: []Entry{{Name: "src", Kind: "directory"}, {Name: "a.go", Kind: "file"}, {Name: "link", Kind: "symlink"}}}
		for i, want := range []bool{true, false, false} {
			if Selectable(Directory, l.Entries[i]) != want || !Selectable(File, l.Entries[i]) || !Selectable(Files, l.Entries[i]) {
				t.Errorf("%s is selectable in the wrong modes", l.Entries[i].Name)
			}
		}
		for _, c := range []struct {
			mode     Mode
			selected []int
			want     []string
		}{
			{Directory, nil, []string{"/work"}},
			{Directory, []int{0}, []string{"/work/src"}},
			{Directory, []int{1}, nil},
			{File, nil, nil},
			{File, []int{0}, nil},
			{File, []int{1}, []string{"/work/a.go"}},
			{Files, []int{0, 1, 2}, []string{"/work/a.go"}},
		} {
			if got := Resolve(c.mode, l, c.selected); !slices.Equal(got, c.want) {
				t.Errorf("Resolve(%d, %v) = %q, want %q", c.mode, c.selected, got, c.want)
			}
		}
		// A name that only starts with a dot has no extension to name.
		if key, extension := KindKey(Entry{Name: ".gitignore", Kind: "file"}); key != "directory_file_kind" || extension != "" {
			t.Errorf(".gitignore is a %s %q", key, extension)
		}
		if key, extension := KindKey(Entry{Name: "a.deb", Kind: "file"}); key != "directory_extension_kind" || extension != "DEB" {
			t.Errorf("a.deb is a %s %q", key, extension)
		}
	})

	// The sidebar marks the longest location holding the directory, by
	// whole path steps.
	t.Run("nearest location", func(t *testing.T) {
		locations := []Location{{Kind: "root", Path: "/"}, {Kind: "home", Path: "/home/me"}, {Kind: "project", Path: "/home/me/work"}, {Path: ""}}
		for path, want := range map[string]string{
			"/": "/", "/usr/lib": "/", "/home/me": "/home/me", "/home/me/notes": "/home/me", "/home/me/work": "/home/me/work",
			"/home/me/work/src": "/home/me/work", "/home/me/workshop": "/home/me", "/home/meadow": "/",
		} {
			if got, ok := Nearest(Listing{Separator: "/", Path: path}, locations); !ok || got != want {
				t.Errorf("Nearest(%s) = %q, want %q", path, got, want)
			}
		}
		if got, ok := Nearest(Listing{Separator: `\`, Path: `D:\data`}, []Location{{Path: `C:\`}, {Path: `D:\`}}); !ok || got != `D:\` {
			t.Errorf(`Nearest(D:\data) = %q`, got)
		}
		if _, ok := Nearest(Listing{Separator: `\`, Path: `D:\data`}, []Location{{Path: `C:\`}}); ok {
			t.Error("a directory under no location has a nearest one")
		}
	})

	t.Run("view", views)

	t.Run("tile classes", func(t *testing.T) {
		for name, want := range map[string]string{
			"main.go": "code", "notes.md": "text", "paper.PDF": "pdf", "photo.jpeg": "image", "data.csv": "sheet",
			"backup.tar": "archive", "song.mp3": "audio", "clip.mov": "video", "Makefile": "file", ".gitignore": "file",
			"code_amd64.deb": "package", "app.dmg": "package", "disk.iso": "archive", "old.7z": "archive", "book.xls": "sheet",
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
