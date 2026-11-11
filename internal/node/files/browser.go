package files

import (
	"context"
	"errors"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/disk"
	"io/fs"
	"os"
	"os/user"
	"path/filepath"
	"runtime"
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
	// A directory that is not there, or that the Node may not reach, is told
	// from a Node that cannot answer.
	switch {
	case errors.Is(e, fs.ErrNotExist):
		return out, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: e.Error()}
	case errors.Is(e, fs.ErrPermission):
		return out, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: e.Error()}
	case e != nil:
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
	// The places and devices go with a directory's first page.
	if after != nil {
		return out, nil
	}
	seen := map[string]bool{}
	add := func(location protocol.FileBrowserLocation) {
		if location.Path == "" || seen[location.Path] || !utf8.ValidString(location.Path) || !utf8.ValidString(location.Name) {
			return
		}
		if m, e := os.Stat(location.Path); e != nil || !m.IsDir() || !readable(location.Path) {
			return
		}
		seen[location.Path] = true
		out.Locations = append(out.Locations, location)
	}
	// The standard folders that exist, as the system places them; one set to
	// the home itself adds nothing.
	if home != "" {
		add(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindHome, Path: home})
		for _, entry := range systemPlaces(home) {
			add(protocol.FileBrowserLocation{Kind: entry.kind, Path: entry.path})
		}
	}
	// The devices, and the root of the directory shown where they do not
	// hold it. Mounts that cannot be listed leave that root alone.
	if mounts, e := disk.PartitionsWithContext(ctx, false); e == nil || len(mounts) > 0 {
		account := ""
		if current, e := user.Current(); e == nil {
			account = current.Username
		}
		startup := ""
		if runtime.GOOS == "darwin" {
			startup = startupName("/Volumes")
		}
		for _, volume := range volumes(runtime.GOOS, mounts, account, startup, driveDetail) {
			add(volume)
		}
	}
	add(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindRoot, Path: filepath.VolumeName(path) + string(filepath.Separator)})
	return out, nil
}
