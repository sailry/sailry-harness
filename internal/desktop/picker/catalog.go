package picker

import (
	"fmt"
	"slices"
	"strings"
	"time"
)

// CatalogNow is the clock of the preview's tree: its entries' times are set
// against it and the picker shows them against it, so the sample's today
// stays today.
var CatalogNow = time.Date(2026, 11, 9, 12, 0, 0, 0, time.UTC)

// sample is an entry of the preview's tree: a directory, or a file of a size.
type sample struct {
	file     bool
	size     uint64
	modified time.Time
}

// Catalog is the preview's file tree, kept in memory: the preview never reads
// the host's disk, and its changes are forgotten with the picker. It is laid
// out as a machine is, so the demo shows nothing that reads as a sample: a
// root volume, an account's home with its standard folders, and a second
// volume.
type Catalog struct {
	entries   map[string]sample // by path
	home      string
	locations []Location
}

// NewCatalog makes the preview tree of the local host, a Mac, or of a remote
// one, a Linux machine. The home holds what a downloads folder does
// (packages, archives, documents and images, with the long names and the
// sizes real ones have) beside the standard folders and a small project.
func NewCatalog(remote bool) *Catalog {
	home, project, scratch := "/Users/alex", "sailry", "scratch"
	if remote {
		home, project, scratch = "/home/alex", "remote-workspace", "remote-scratch"
	}
	c := &Catalog{entries: map[string]sample{}, home: home}
	const hour, day = time.Hour, 24 * time.Hour
	directory := func(path string, age time.Duration) {
		c.entries[path] = sample{modified: CatalogNow.Add(-age)}
	}
	file := func(path string, size uint64, age time.Duration) {
		c.entries[path] = sample{file: true, size: size, modified: CatalogNow.Add(-age)}
	}
	directory("/", 200*day)
	directory(home[:strings.LastIndex(home, "/")], 200*day)
	directory(home, 2*hour)
	c.locations = []Location{{Kind: "home", Path: home}}
	place := func(kind, path string, age time.Duration) {
		directory(path, age)
		c.locations = append(c.locations, Location{Kind: kind, Path: path})
	}
	if remote {
		place("documents", home+"/Documents", 12*day)
		place("downloads", home+"/Downloads", 2*day)
		directory("/mnt", 200*day)
		directory("/mnt/data", 5*day)
		directory("/mnt/data/datasets", 5*day)
		c.locations = append(c.locations, Location{Kind: "root", Path: "/", Medium: "internal"}, Location{Kind: "volume", Name: "data", Path: "/mnt/data"})
	} else {
		place("desktop", home+"/Desktop", 3*hour)
		place("documents", home+"/Documents", 12*day)
		place("downloads", home+"/Downloads", 2*day)
		place("applications", "/Applications", 20*day)
		for _, name := range []string{"Calculator.app", "Sailry.app", "Terminal.app", "Utilities"} {
			directory("/Applications/"+name, 20*day)
		}
		directory("/Volumes", 200*day)
		directory("/Volumes/Backup", 27*hour)
		directory("/Volumes/Backup/Archive", 27*hour)
		file("/Volumes/Backup/backup-2026-11-08.zip", 1_200_000_000, 27*hour)
		file(home+"/Desktop/meeting-notes.md", 3_400, 3*hour)
		c.locations = append(c.locations, Location{Kind: "root", Name: "Macintosh HD", Path: "/", Medium: "internal"},
			Location{Kind: "volume", Name: "Backup", Path: "/Volumes/Backup", Medium: "external"})
	}
	file(home+"/Documents/budget-2026.xlsx", 48_000, 12*day)
	file(home+"/Downloads/installer.pkg", 52_000_000, 2*day)
	project, scratch = home+"/"+project, home+"/"+scratch
	directory(project, 40*time.Minute)
	directory(scratch, 3*day)
	directory(home+"/empty", 21*day)
	directory(home+"/restricted", 60*day)
	directory(home+"/many", 9*day)
	directory(project+"/src", 40*time.Minute)
	directory(project+"/docs", 26*hour)
	directory(scratch+"/new-project", 3*day)
	for i := range 64 {
		directory(fmt.Sprintf("%s/many/directory-%02d-with-a-long-preview-name", home, i), 9*day)
	}
	file(home+"/.DS_Store", 6_148, 2*hour)
	file(home+"/architecture-review.pdf", 2_300_000, 5*hour)
	file(home+"/backup-2026-11-08.zip", 1_200_000_000, 27*hour)
	file(home+"/banner.png", 1_800_000, 4*day)
	file(home+"/code_1.96.2_amd64.deb", 104_600_000, 12*day)
	file(home+"/go1.27.3.darwin-arm64.tar.gz", 78_400_000, 18*day)
	file(home+"/release-notes.md", 6_200, 90*time.Minute)
	file(home+"/sailry-desktop-0.1.0.dmg", 96_000_000, 30*hour)
	file(home+"/screenshot-2026-11-08.png", 640_000, 25*hour)
	file(home+"/WorkBuddy-linux-x64-deb-5.6.2.40046327-c7b525f3.deb", 429_800_000, 47*day)
	file(home+"/产品需求文档 v3.docx", 284_000, 6*day)
	file(home+"/季度财务报表.xlsx", 1_100_000, 400*day)
	file(project+"/.gitignore", 312, 30*day)
	file(project+"/config.yaml", 824, 8*day)
	file(project+"/deploy.sh", 1_300, 15*day)
	file(project+"/go.mod", 1_900, 26*hour)
	file(project+"/main.go", 4_100, 40*time.Minute)
	file(project+"/Makefile", 2_000, 15*day)
	file(project+"/README.md", 6_200, 3*hour)
	file(project+"/src/app.go", 12_400, 40*time.Minute)
	file(project+"/src/app_test.go", 8_700, 50*time.Minute)
	file(project+"/docs/architecture.pdf", 2_300_000, 26*hour)
	file(project+"/docs/guide.md", 9_600, 5*day)
	file(scratch+"/notes.txt", 2_048, 3*day)
	return c
}

// Home is the home directory of the tree's account, where its picker opens.
func (c *Catalog) Home() string { return c.home }

// Expand is path with a leading "~" as the account's home, the way the
// sample projects write their directories.
func (c *Catalog) Expand(path string) string {
	if path == "~" || strings.HasPrefix(path, "~/") {
		return c.home + path[1:]
	}
	return path
}

// Add puts a directory, or a file of a size, in the tree with the
// directories that lead to it: the sample projects and their files, so the
// picker shows what the window's own data names.
func (c *Catalog) Add(path string, file bool, size uint64, modified time.Time) {
	for dir := parent(path); dir != ""; dir = parent(dir) {
		if _, ok := c.entries[dir]; !ok {
			c.entries[dir] = sample{modified: modified}
		}
	}
	c.entries[path] = sample{file: file, size: size, modified: modified}
}

// Size is the size of the file at path, 0 for a directory or nothing.
func (c *Catalog) Size(path string) uint64 { return c.entries[path].size }

// Source is the catalog as a picker's host.
func (c *Catalog) Source() Source {
	return Source{
		Browse: func(directory string, done func(Listing, string)) {
			if directory == "" {
				directory = c.home
			}
			done(c.List(directory))
		},
		Manage: func(action Action, done func(string)) { done(c.Apply(action)) },
	}
}

// directory reports whether path is a directory of the tree.
func (c *Catalog) directory(path string) bool {
	entry, ok := c.entries[path]
	return ok && !entry.file
}

// parent is the directory holding path, "" for the root.
func parent(path string) string {
	switch i := strings.LastIndex(path, "/"); {
	case path == "/" || i < 0:
		return ""
	case i == 0:
		return "/"
	default:
		return path[:i]
	}
}

// List is a directory of the tree, or the i18n key of why it cannot show.
func (c *Catalog) List(path string) (Listing, string) {
	if path = strings.TrimSpace(path); path != "/" {
		path = strings.TrimRight(path, "/")
	}
	if path == c.home+"/restricted" {
		return Listing{}, "directory_denied"
	}
	if !c.directory(path) {
		return Listing{}, "directory_missing"
	}
	l := Listing{Separator: "/", Path: path, Parent: parent(path), Locations: slices.Clone(c.locations)}
	for child, entry := range c.entries {
		if parent(child) == path {
			kind := "directory"
			if entry.file {
				kind = "file"
			}
			l.Entries = append(l.Entries, Entry{Name: child[strings.LastIndex(child, "/")+1:], Kind: kind, Size: entry.size, Modified: entry.modified})
		}
	}
	slices.SortFunc(l.Entries, func(a, b Entry) int { return strings.Compare(a.Name, b.Name) })
	return l, ""
}

// Apply changes the tree in memory and returns the i18n key of a failure.
func (c *Catalog) Apply(a Action) string {
	switch a.Kind {
	case "create":
		if problem := c.destination(a.Path); problem != "" {
			return problem
		}
		c.entries[a.Path] = sample{modified: CatalogNow}
	case "rename":
		if problem := c.destination(a.To); problem != "" {
			return problem
		}
		return c.transfer(a.Path, a.To, true)
	case "trash":
		for _, path := range a.Paths {
			if problem := c.remove(path); problem != "" {
				return problem
			}
		}
	case "transfer":
		if _, problem := c.List(a.Destination); problem != "" {
			return problem
		}
		for _, path := range a.Paths {
			i := strings.LastIndex(path, "/")
			if i < 0 {
				return "directory_missing"
			}
			name := path[i+1:]
			into := strings.TrimRight(a.Destination, "/")
			target := into + "/" + name
			if _, taken := c.entries[target]; taken {
				switch a.Conflict {
				case "replace":
					if path == target || strings.HasPrefix(path, target+"/") {
						return "files_paste_conflict"
					}
					if problem := c.remove(target); problem != "" {
						return problem
					}
				case "keep_both":
					for index := 2; taken; index++ {
						target = fmt.Sprintf("%s/%s (%d)", into, name, index)
						_, taken = c.entries[target]
					}
				default:
					return "files_paste_conflict"
				}
			}
			if problem := c.transfer(path, target, a.Cut); problem != "" {
				return problem
			}
		}
	}
	return ""
}

func (c *Catalog) destination(path string) string {
	i := strings.LastIndex(path, "/")
	if i < 0 {
		return "directory_missing"
	}
	if _, taken := c.entries[path]; taken {
		return "files_paste_conflict"
	}
	if name := path[i+1:]; name == "" || name == "." || name == ".." {
		return "files_paste_conflict"
	}
	_, problem := c.List(parent(path))
	return problem
}

func (c *Catalog) transfer(from, to string, cut bool) string {
	if _, ok := c.entries[from]; !ok || strings.HasPrefix(to, from+"/") {
		return "files_paste_conflict"
	}
	copied := map[string]sample{}
	for path, entry := range c.entries {
		if path == from || strings.HasPrefix(path, from+"/") {
			copied[to+path[len(from):]] = entry
		}
	}
	if cut {
		if problem := c.remove(from); problem != "" {
			return problem
		}
	}
	for path, entry := range copied {
		c.entries[path] = entry
	}
	return ""
}

func (c *Catalog) remove(path string) string {
	if _, ok := c.entries[path]; path == "/" || path == c.home || path == c.home+"/restricted" || !ok {
		return "directory_denied"
	}
	for entry := range c.entries {
		if entry == path || strings.HasPrefix(entry, path+"/") {
			delete(c.entries, entry)
		}
	}
	return ""
}
