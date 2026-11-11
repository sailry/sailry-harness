// Package picker is the in-app file and directory chooser. It browses the
// file system of one execution host through a Source, so a remote Host is
// browsed exactly as the local Node is, and never reads the controller's disk
// itself.
package picker

import (
	"cmp"
	"fmt"
	"slices"
	"strings"
	"time"
)

// Mode is what the picker chooses.
type Mode int

const (
	// Directory chooses the directory shown, or the one directory selected in it.
	Directory Mode = iota
	// File chooses one file.
	File
	// Files chooses one or more files.
	Files
)

// Entry is a directory's child.
type Entry struct {
	Name string
	// Kind is "directory", "file", "symlink" or "other"; only directories and
	// files can be selected.
	Kind string
	Size uint64
	// Modified is when the entry last changed, zero where its host does not
	// say.
	Modified time.Time
}

// Dir reports whether the entry is a directory.
func (e Entry) Dir() bool { return e.Kind == "directory" }

// Location is a place the host offers in the sidebar: Kind is home, desktop,
// documents, downloads, volume or root.
type Location struct{ Kind, Name, Path string }

// Listing is one directory as its host reports it.
type Listing struct {
	Separator string
	Path      string
	Parent    string // "" at a root
	Entries   []Entry
	Locations []Location
}

// Action is a change to the host's files: Kind is create, rename, transfer or
// trash.
type Action struct {
	Kind        string
	Path, To    string   // create: Path; rename: Path to To
	Paths       []string // transfer and trash
	Destination string   // transfer
	Cut         bool
	Conflict    string // stop, replace or keep_both
}

// Source reads and changes one host's files. Both callbacks run on the UI
// thread, at once or later.
type Source struct {
	// Browse lists directory, the host's home for "". A failure is an i18n key.
	Browse func(directory string, done func(Listing, string))
	// Manage applies action; problem is an i18n key, "" on success.
	Manage func(action Action, done func(problem string))
}

// Visible returns the indexes of the entries the hidden toggle and the filter
// keep, in the directory's order.
func Visible(entries []Entry, hidden bool, filter string) []int {
	filter = strings.ToLower(filter)
	var out []int
	for i, e := range entries {
		if (hidden || !strings.HasPrefix(e.Name, ".")) && strings.Contains(strings.ToLower(e.Name), filter) {
			out = append(out, i)
		}
	}
	return out
}

// Order sorts visible indexes: directories first, then by the column (name,
// kind, size or modified) and the lower-cased name; descending reverses the
// files and the directories among themselves. An entry without a modified
// time is older than any with one.
func Order(entries []Entry, visible []int, column string, descending bool, kind func(Entry) string) {
	slices.SortStableFunc(visible, func(a, b int) int {
		x, y := entries[a], entries[b]
		if x.Dir() != y.Dir() {
			if x.Dir() {
				return -1
			}
			return 1
		}
		order := 0
		switch column {
		case "kind":
			order = strings.Compare(kind(x), kind(y))
		case "size":
			order = cmp.Compare(x.Size, y.Size)
		case "modified":
			order = x.Modified.Compare(y.Modified)
		}
		if order == 0 {
			order = strings.Compare(strings.ToLower(x.Name), strings.ToLower(y.Name))
		}
		if order == 0 {
			order = strings.Compare(x.Name, y.Name)
		}
		if descending {
			order = -order
		}
		return order
	})
}

// Join is a child's path in the listing's directory.
func (l Listing) Join(name string) string {
	return strings.TrimRight(l.Path, l.Separator) + l.Separator + name
}

// Selectable reports whether the mode lets e be selected: a directory picker
// selects directories only, and shows the files beside them for bearings.
func Selectable(mode Mode, e Entry) bool { return mode != Directory || e.Dir() }

// Resolve is what Confirm would choose, nil for nothing: in Directory mode the
// directory shown, or the one directory selected; in the file modes the
// selected files, exactly one for File.
func Resolve(mode Mode, l Listing, selected []int) []string {
	if mode == Directory {
		if len(selected) == 0 {
			return []string{l.Path}
		}
		if len(selected) != 1 || selected[0] >= len(l.Entries) || !l.Entries[selected[0]].Dir() {
			return nil
		}
		return []string{l.Join(l.Entries[selected[0]].Name)}
	}
	var files []string
	for _, i := range selected {
		if i < len(l.Entries) && l.Entries[i].Kind == "file" {
			files = append(files, l.Join(l.Entries[i].Name))
		}
	}
	if len(files) == 0 || mode == File && (len(selected) != 1 || len(files) != 1) {
		return nil
	}
	return files
}

// Nearest is the path of the location the listing's directory is, or is
// inside: the longest one, so a project under the home marks the project.
// It is "", false when no location holds the directory.
func Nearest(l Listing, locations []Location) (string, bool) {
	best, found := "", false
	for _, loc := range locations {
		rest, under := strings.CutPrefix(l.Path, loc.Path)
		if !under || loc.Path == "" || rest != "" && !strings.HasPrefix(rest, l.Separator) && !strings.HasSuffix(loc.Path, l.Separator) {
			continue
		}
		if !found || len(loc.Path) > len(best) {
			best, found = loc.Path, true
		}
	}
	return best, found
}

// Crumb is a step of the address: its label and the path it opens.
type Crumb struct{ Label, Path string }

// Crumbs splits the listing's path at the host's separator. The first crumb
// is the volume or root holding the path, as the host names it; a UNC root is
// its server and share.
func Crumbs(l Listing) []Crumb {
	path, sep := l.Path, l.Separator
	root := "/"
	if sep == `\` {
		root, _, _ = strings.Cut(path, sep)
	}
	label := root
	if sep == `\` && root != "" {
		root += sep
	}
	best := -1
	for _, loc := range l.Locations {
		if loc.Kind != "volume" && loc.Kind != "root" {
			continue
		}
		rest, under := strings.CutPrefix(path, loc.Path)
		if !under || rest != "" && !strings.HasPrefix(rest, sep) && !strings.HasSuffix(loc.Path, sep) {
			continue
		}
		if len(loc.Path) >= best {
			best, root = len(loc.Path), loc.Path
			if loc.Name != "" {
				label = loc.Name
			}
		}
	}
	if sep == `\` && root == "" {
		var parts []string
		for _, part := range strings.Split(path, sep) {
			if part != "" && len(parts) < 2 {
				parts = append(parts, part)
			}
		}
		root = `\\` + strings.Join(parts, `\`)
		label = root
	}
	out := []Crumb{{label, root}}
	current := strings.TrimRight(root, sep)
	for _, part := range strings.Split(strings.TrimPrefix(path, root), sep) {
		if part != "" {
			current += sep + part
			out = append(out, Crumb{part, current})
		}
	}
	return out
}

// KindKey is the i18n key naming an entry's kind, and for
// directory_extension_kind the upper-cased extension it takes. A name that
// only starts with a dot (.gitignore) has no extension.
func KindKey(e Entry) (key, extension string) {
	if e.Dir() {
		return "directory_folder_kind", ""
	}
	if i := strings.LastIndex(e.Name, "."); i > 0 {
		extension = strings.ToLower(e.Name[i+1:])
	}
	switch extension {
	case "md", "markdown":
		return "directory_markdown_kind", ""
	case "toml":
		return "directory_toml_kind", ""
	case "json":
		return "directory_json_kind", ""
	case "yaml", "yml":
		return "directory_yaml_kind", ""
	case "txt":
		return "directory_text_kind", ""
	case "png", "jpg", "jpeg", "gif", "webp":
		return "directory_image_kind", ""
	case "":
		return "directory_file_kind", ""
	}
	return "directory_extension_kind", strings.ToUpper(extension)
}

// Bytes is an entry's size in decimal units, a dash for a directory.
func Bytes(e Entry) string {
	if e.Dir() {
		return "—"
	}
	switch {
	case e.Size >= 1_000_000_000:
		return fmt.Sprintf("%.1f GB", float64(e.Size)/1e9)
	case e.Size >= 1_000_000:
		return fmt.Sprintf("%.1f MB", float64(e.Size)/1e6)
	case e.Size >= 1_000:
		return fmt.Sprintf("%.1f KB", float64(e.Size)/1e3)
	}
	return fmt.Sprintf("%d B", e.Size)
}

// Day places a modification time against now, in now's time zone: 0 on the
// same day, 1 on the day before, 2 on any other, and -1 for no time. The
// list shows the first two with the time of day and the rest as a date.
func Day(modified, now time.Time) int {
	if modified.IsZero() {
		return -1
	}
	year, month, day := now.Date()
	today := time.Date(year, month, day, 0, 0, 0, 0, now.Location())
	switch {
	case !modified.Before(today) && modified.Before(today.AddDate(0, 0, 1)):
		return 0
	case !modified.Before(today.AddDate(0, 0, -1)) && modified.Before(today):
		return 1
	}
	return 2
}

// ValidName reports whether name can be a new entry's: not empty, not . or ..,
// without a separator or NUL.
func ValidName(name string) bool {
	name = strings.TrimSpace(name)
	return name != "" && name != "." && name != ".." && !strings.ContainsAny(name, "/\\\x00")
}
