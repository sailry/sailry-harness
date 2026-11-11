package picker

import (
	"fmt"
	"slices"
	"strings"
)

// Catalog is the preview's file tree, kept in memory: the preview never reads
// the host's disk, and its changes are forgotten with the picker.
type Catalog struct {
	entries map[string]bool // directories by path
	home    string
}

// NewCatalog makes the preview tree of the local host, or of a remote one.
func NewCatalog(remote bool) *Catalog {
	home, scratch := "/preview/sailry", "/preview/scratch"
	if remote {
		home, scratch = "/preview/remote-workspace", "/preview/remote-scratch"
	}
	c := &Catalog{entries: map[string]bool{}, home: home}
	for _, path := range []string{"/preview", home, scratch, "/preview/empty", "/preview/restricted", "/preview/many",
		home + "/src", home + "/docs", scratch + "/new-project"} {
		c.entries[path] = true
	}
	for i := range 64 {
		c.entries[fmt.Sprintf("/preview/many/directory-%02d-with-a-long-preview-name", i)] = true
	}
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

// List is a directory of the tree, or the i18n key of why it cannot show.
func (c *Catalog) List(path string) (Listing, string) {
	path = strings.TrimRight(strings.TrimSpace(path), "/")
	if path == "/preview/restricted" {
		return Listing{}, "directory_denied"
	}
	if !c.entries[path] {
		return Listing{}, "directory_missing"
	}
	l := Listing{Separator: "/", Path: path, Locations: []Location{{Kind: "root", Path: "/preview"}, {Kind: "home", Path: c.home}}}
	if path != "/preview" {
		l.Parent = path[:strings.LastIndex(path, "/")]
	}
	for entry := range c.entries {
		if i := strings.LastIndex(entry, "/"); i > 0 && entry[:i] == path {
			l.Entries = append(l.Entries, Entry{Name: entry[i+1:], Kind: "directory"})
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
		c.entries[a.Path] = true
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
			if c.entries[target] {
				switch a.Conflict {
				case "replace":
					if path == target || strings.HasPrefix(path, target+"/") {
						return "files_paste_conflict"
					}
					if problem := c.remove(target); problem != "" {
						return problem
					}
				case "keep_both":
					for index := 2; c.entries[target]; index++ {
						target = fmt.Sprintf("%s/%s (%d)", a.Destination, name, index)
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
	if name := path[i+1:]; name == "" || name == "." || name == ".." || c.entries[path] {
		return "files_paste_conflict"
	}
	_, problem := c.List(path[:i])
	return problem
}

func (c *Catalog) transfer(from, to string, cut bool) string {
	if !c.entries[from] || strings.HasPrefix(to, from+"/") {
		return "files_paste_conflict"
	}
	var copied []string
	for path := range c.entries {
		if path == from || strings.HasPrefix(path, from+"/") {
			copied = append(copied, to+path[len(from):])
		}
	}
	if cut {
		if problem := c.remove(from); problem != "" {
			return problem
		}
	}
	for _, path := range copied {
		c.entries[path] = true
	}
	return ""
}

func (c *Catalog) remove(path string) string {
	if path == "/preview" || path == "/preview/restricted" || !c.entries[path] {
		return "directory_denied"
	}
	for entry := range c.entries {
		if entry == path || strings.HasPrefix(entry, path+"/") {
			delete(c.entries, entry)
		}
	}
	return ""
}
