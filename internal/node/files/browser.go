package files

import (
	"context"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"os"
	"path/filepath"
	"unicode/utf8"
)

func Browse(ctx context.Context, directory *string, after *protocol.DirectoryCursor) (protocol.FileBrowserListing, error) {
	out := protocol.DefaultFileBrowserListing()
	out.Separator = protocol.Char(string(filepath.Separator))
	home, e := os.UserHomeDir()
	if e != nil && directory == nil {
		return out, e
	}
	path := home
	if directory != nil {
		path = *directory
	}
	if !filepath.IsAbs(path) || !utf8.ValidString(path) {
		return out, invalid("an absolute UTF-8 path is required")
	}
	path, e = filepath.EvalSymlinks(path)
	if e != nil {
		return out, e
	}
	files, e := core.OpenFiles(path, core.ManualFiles)
	if e != nil {
		return out, e
	}
	defer files.Close()
	out.Directory, e = files.ListDirectory(ctx, "", after)
	if e != nil {
		return out, e
	}
	out.Directory.Path = path
	parent := filepath.Dir(path)
	if parent != path {
		out.Parent = &parent
	}
	seen := map[string]bool{}
	add := func(kind protocol.FileBrowserLocationKind, name, path string) {
		if seen[path] || !utf8.ValidString(path) {
			return
		}
		if m, e := os.Stat(path); e != nil || !m.IsDir() {
			return
		}
		seen[path] = true
		out.Locations = append(out.Locations, protocol.FileBrowserLocation{Kind: kind, Name: name, Path: path})
	}
	add(protocol.FileBrowserLocationKindHome, "", home)
	for _, entry := range []struct {
		kind protocol.FileBrowserLocationKind
		name string
	}{{protocol.FileBrowserLocationKindDesktop, "Desktop"}, {protocol.FileBrowserLocationKindDocuments, "Documents"}, {protocol.FileBrowserLocationKindDownloads, "Downloads"}} {
		add(entry.kind, "", filepath.Join(home, entry.name))
	}
	root := filepath.VolumeName(path) + string(filepath.Separator)
	add(protocol.FileBrowserLocationKindRoot, "", root)
	volumes, e := browserVolumes()
	if e != nil {
		return out, e
	}
	for _, volume := range volumes {
		add(volume.Kind, volume.Name, volume.Path)
	}
	return out, nil
}
