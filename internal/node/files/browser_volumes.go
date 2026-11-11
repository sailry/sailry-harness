package files

import (
	"os"
	"path/filepath"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/shirou/gopsutil/v4/disk"
)

// volumes are the devices a file manager of goos shows, out of the mounts
// the host metrics list (gopsutil's partitions, which leave out the pseudo
// file systems): on macOS the startup volume under its name and the
// browsable volumes mounted in /Volumes; on Linux the root and the media
// mounted for account under /media and /run/media, and in /mnt; on Windows
// the drive letters. detail names a Windows drive and tells its medium.
func volumes(goos string, mounts []disk.PartitionStat, account, startup string, detail func(root string) (string, *protocol.FileBrowserMedium)) []protocol.FileBrowserLocation {
	medium := func(value protocol.FileBrowserMedium) *protocol.FileBrowserMedium { return &value }
	var out []protocol.FileBrowserLocation
	seen := map[string]bool{}
	add := func(location protocol.FileBrowserLocation) {
		if !seen[location.Path] {
			seen[location.Path] = true
			out = append(out, location)
		}
	}
	if goos != "windows" {
		for _, mount := range mounts {
			if mount.Mountpoint == "/" {
				add(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindRoot, Name: startup, Path: "/", Medium: medium(protocol.FileBrowserMediumInternal)})
			}
		}
	}
	for _, mount := range mounts {
		path := mount.Mountpoint
		switch goos {
		case "windows":
			if len(path) != 2 || path[1] != ':' {
				continue
			}
			root := path + `\`
			label, kind := detail(root)
			name := path
			if label != "" {
				name = label + " (" + path + ")"
			}
			add(protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: name, Path: root, Medium: kind})
		case "darwin":
			name := filepath.Base(path)
			if filepath.Dir(path) != "/Volumes" || strings.HasPrefix(name, ".") || slices.Contains(mount.Opts, "nobrowse") {
				continue
			}
			location := protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: name, Path: path}
			if !slices.Contains(mount.Opts, "local") {
				location.Medium = medium(protocol.FileBrowserMediumNetwork)
			}
			add(location)
		case "linux":
			parent, name := filepath.Dir(path), filepath.Base(path)
			location := protocol.FileBrowserLocation{Kind: protocol.FileBrowserLocationKindVolume, Name: name, Path: path}
			switch {
			case strings.HasPrefix(name, "."):
				continue
			case account != "" && (parent == "/media/"+account || parent == "/run/media/"+account):
				location.Medium = medium(protocol.FileBrowserMediumExternal)
			case parent != "/mnt":
				continue
			}
			add(location)
		}
	}
	return out
}

// startupName is the name of the startup volume of macOS: the link to / the
// system keeps beside the other volumes, in directory (/Volumes). It is ""
// where there is none.
func startupName(directory string) string {
	entries, err := os.ReadDir(directory)
	if err != nil {
		return ""
	}
	for _, entry := range entries {
		if entry.Type()&os.ModeSymlink == 0 {
			continue
		}
		if target, err := os.Readlink(filepath.Join(directory, entry.Name())); err == nil && target == "/" {
			return entry.Name()
		}
	}
	return ""
}
