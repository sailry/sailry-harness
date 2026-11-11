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
// the host's disk, and its changes are forgotten with the picker.
type Catalog struct {
	entries map[string]sample // by path
	home    string
}

// NewCatalog makes the preview tree of the local host, or of a remote one:
// its root holds what a downloads folder does (packages, archives, documents
// and images, with the long names and the sizes real ones have), its home a
// small project.
func NewCatalog(remote bool) *Catalog {
	home, scratch := "/preview/sailry", "/preview/scratch"
	if remote {
		home, scratch = "/preview/remote-workspace", "/preview/remote-scratch"
	}
	c := &Catalog{entries: map[string]sample{}, home: home}
	const hour, day = time.Hour, 24 * time.Hour
	directory := func(path string, age time.Duration) {
		c.entries[path] = sample{modified: CatalogNow.Add(-age)}
	}
	file := func(path string, size uint64, age time.Duration) {
		c.entries[path] = sample{file: true, size: size, modified: CatalogNow.Add(-age)}
	}
	directory("/preview", 2*hour)
	directory(home, 40*time.Minute)
	directory(scratch, 3*day)
	directory("/preview/empty", 21*day)
	directory("/preview/restricted", 60*day)
	directory("/preview/many", 9*day)
	directory(home+"/src", 40*time.Minute)
	directory(home+"/docs", 26*hour)
	directory(scratch+"/new-project", 3*day)
	for i := range 64 {
		directory(fmt.Sprintf("/preview/many/directory-%02d-with-a-long-preview-name", i), 9*day)
	}
	file("/preview/.DS_Store", 6_148, 2*hour)
	file("/preview/architecture-review.pdf", 2_300_000, 5*hour)
	file("/preview/backup-2026-11-08.zip", 1_200_000_000, 27*hour)
	file("/preview/banner.png", 1_800_000, 4*day)
	file("/preview/code_1.96.2_amd64.deb", 104_600_000, 12*day)
	file("/preview/go1.27.3.darwin-arm64.tar.gz", 78_400_000, 18*day)
	file("/preview/release-notes.md", 6_200, 90*time.Minute)
	file("/preview/sailry-desktop-0.1.0.dmg", 96_000_000, 30*hour)
	file("/preview/screenshot-2026-11-08.png", 640_000, 25*hour)
	file("/preview/WorkBuddy-linux-x64-deb-5.6.2.40046327-c7b525f3.deb", 429_800_000, 47*day)
	file("/preview/产品需求文档 v3.docx", 284_000, 6*day)
	file("/preview/季度财务报表.xlsx", 1_100_000, 400*day)
	file(home+"/.gitignore", 312, 30*day)
	file(home+"/config.yaml", 824, 8*day)
	file(home+"/deploy.sh", 1_300, 15*day)
	file(home+"/go.mod", 1_900, 26*hour)
	file(home+"/main.go", 4_100, 40*time.Minute)
	file(home+"/Makefile", 2_000, 15*day)
	file(home+"/README.md", 6_200, 3*hour)
	file(home+"/src/app.go", 12_400, 40*time.Minute)
	file(home+"/src/app_test.go", 8_700, 50*time.Minute)
	file(home+"/docs/architecture.pdf", 2_300_000, 26*hour)
	file(home+"/docs/guide.md", 9_600, 5*day)
	file(scratch+"/notes.txt", 2_048, 3*day)
	return c
}

// Source is the catalog as a picker's host.
func (c *Catalog) Source() Source {
	return Source{
		Browse: func(directory string, done func(Listing, string)) {
			if directory == "" {
				directory = "/preview"
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

// List is a directory of the tree, or the i18n key of why it cannot show.
func (c *Catalog) List(path string) (Listing, string) {
	path = strings.TrimRight(strings.TrimSpace(path), "/")
	if path == "/preview/restricted" {
		return Listing{}, "directory_denied"
	}
	if !c.directory(path) {
		return Listing{}, "directory_missing"
	}
	l := Listing{Separator: "/", Path: path, Locations: []Location{{Kind: "root", Path: "/preview"}, {Kind: "home", Path: c.home}}}
	if path != "/preview" {
		l.Parent = path[:strings.LastIndex(path, "/")]
	}
	for child, entry := range c.entries {
		if i := strings.LastIndex(child, "/"); i > 0 && child[:i] == path {
			kind := "directory"
			if entry.file {
				kind = "file"
			}
			l.Entries = append(l.Entries, Entry{Name: child[i+1:], Kind: kind, Size: entry.size, Modified: entry.modified})
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
			target := a.Destination + "/" + name
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
						target = fmt.Sprintf("%s/%s (%d)", a.Destination, name, index)
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
	_, problem := c.List(path[:i])
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
	if _, ok := c.entries[path]; path == "/preview" || path == "/preview/restricted" || !ok {
		return "directory_denied"
	}
	for entry := range c.entries {
		if entry == path || strings.HasPrefix(entry, path+"/") {
			delete(c.entries, entry)
		}
	}
	return ""
}
